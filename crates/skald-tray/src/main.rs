use std::{
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use ksni::{
    ToolTip, TrayMethods,
    menu::{MenuItem, StandardItem},
};
use skald_core::{
    client,
    config::Config,
    desktop::DesktopStatus,
    protocol::{Command, EventKind, JobState},
};
use tokio::sync::mpsc;
use tracing::warn;

#[derive(Debug, Clone, Copy)]
enum Action {
    Toggle,
    Start,
    Stop,
    Cancel,
    LaunchOverlay,
    CloseOverlay,
    TestMicrophone,
    OpenConfig,
    OpenDocs,
    RestartDaemon,
    Quit,
}

#[derive(Debug)]
struct SkaldTray {
    action_tx: mpsc::UnboundedSender<Action>,
    desktop: DesktopStatus,
    job_state: JobState,
    overlay_mode: String,
    visualizer_style: String,
    microphone: String,
    cleanup_enabled: bool,
}

impl ksni::Tray for SkaldTray {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "skald".into()
    }

    fn title(&self) -> String {
        format!("Skald — {}", self.desktop.tooltip)
    }

    fn icon_name(&self) -> String {
        match self.desktop.class {
            "recording" => "audio-input-microphone-high",
            "error" | "disconnected" => "dialog-warning",
            _ => "audio-input-microphone",
        }
        .into()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            icon_name: self.icon_name(),
            title: "Skald".into(),
            description: self.desktop.tooltip.clone(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let connected = self.desktop.class != "disconnected";
        let recording = matches!(self.job_state, JobState::Recording | JobState::Stopping);
        let mut items = vec![
            disabled(format!("Status: {}", self.desktop.tooltip)),
            MenuItem::Separator,
            action("Toggle recording", connected, Action::Toggle),
            action("Start recording", connected && !recording, Action::Start),
            action("Stop recording", connected && recording, Action::Stop),
            action("Cancel recording", connected && recording, Action::Cancel),
            MenuItem::Separator,
            disabled(format!(
                "Overlay: {} / {}",
                self.overlay_mode, self.visualizer_style
            )),
            action("Launch overlay", true, Action::LaunchOverlay),
            action("Close overlay", true, Action::CloseOverlay),
            MenuItem::Separator,
            disabled(format!("Microphone: {}", self.microphone)),
            action("Test microphone", true, Action::TestMicrophone),
        ];

        let cleanup = if self.cleanup_enabled {
            "Cleanup: enabled — text may leave this machine and incur cost"
        } else {
            "Cleanup: disabled — local processing"
        };
        items.extend([
            disabled(cleanup.into()),
            MenuItem::Separator,
            action("Open configuration", true, Action::OpenConfig),
            action("Open documentation", true, Action::OpenDocs),
            action("Restart daemon", true, Action::RestartDaemon),
            MenuItem::Separator,
            action("Quit tray", true, Action::Quit),
        ]);
        items
    }
}

fn action(label: &str, enabled: bool, action: Action) -> MenuItem<SkaldTray> {
    StandardItem {
        label: label.into(),
        enabled,
        activate: Box::new(move |tray: &mut SkaldTray| {
            let _ = tray.action_tx.send(action);
        }),
        ..Default::default()
    }
    .into()
}

fn disabled(label: String) -> MenuItem<SkaldTray> {
    StandardItem {
        label,
        enabled: false,
        ..Default::default()
    }
    .into()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "skald_tray=info".into()),
        )
        .init();

    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        bail!("no D-Bus user session; skald-tray requires StatusNotifier/AppIndicator support");
    }

    let config = Config::load_validated()?;
    let socket = client::socket_path_from_config()?;
    let (action_tx, mut action_rx) = mpsc::unbounded_channel();
    let tray = SkaldTray {
        action_tx,
        desktop: DesktopStatus::disconnected(),
        job_state: JobState::Idle,
        overlay_mode: config.overlay.mode.clone(),
        visualizer_style: config.overlay.visualizer_style.clone(),
        microphone: config.audio.device.clone(),
        cleanup_enabled: config.cleanup.enabled,
    };
    let handle = tray.spawn().await.context(
        "cannot create tray; install or enable a StatusNotifier/AppIndicator host for this desktop",
    )?;
    let (state_tx, mut state_rx) = mpsc::unbounded_channel();
    tokio::spawn(event_worker(socket.clone(), state_tx));

    loop {
        tokio::select! {
            Some(update) = state_rx.recv() => {
                handle.update(move |tray| {
                    tray.desktop = update.desktop;
                    tray.job_state = update.job_state;
                }).await;
            }
            Some(action) = action_rx.recv() => {
                if matches!(action, Action::Quit) {
                    handle.shutdown().await;
                    return Ok(());
                }
                if let Err(error) = run_action(action, &socket).await {
                    warn!(%error, ?action, "tray action failed");
                }
            }
        }
    }
}

struct StateUpdate {
    desktop: DesktopStatus,
    job_state: JobState,
}

async fn event_worker(socket: PathBuf, tx: mpsc::UnboundedSender<StateUpdate>) {
    let kinds = vec![EventKind::State, EventKind::Result, EventKind::Error];
    let mut backoff = Duration::from_secs(1);
    loop {
        let _ = tx.send(StateUpdate {
            desktop: DesktopStatus::disconnected(),
            job_state: JobState::Idle,
        });
        if let Ok(response) = client::request(&socket, Command::Status).await
            && response.ok
            && let Some(status) = response.status
        {
            let _ = tx.send(StateUpdate {
                desktop: DesktopStatus::from_daemon(&status),
                job_state: status.job_state,
            });
        }

        match client::subscribe(&socket, kinds.clone()).await {
            Ok((response, reader)) if response.ok => {
                backoff = Duration::from_secs(1);
                let mut reader = tokio::io::BufReader::new(reader);
                while let Ok(event) = client::read_event(&mut reader).await {
                    let job_state = match &event {
                        skald_core::protocol::Event::State { job_state, .. } => job_state.clone(),
                        skald_core::protocol::Event::Result { .. } => JobState::Idle,
                        skald_core::protocol::Event::Error { error, .. } => JobState::Failed {
                            code: error.code.clone(),
                            message: error.message.clone(),
                        },
                        _ => continue,
                    };
                    if let Some(desktop) = DesktopStatus::from_event(&event)
                        && tx.send(StateUpdate { desktop, job_state }).is_err()
                    {
                        return;
                    }
                }
            }
            _ => {}
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(15));
    }
}

async fn run_action(action: Action, socket: &Path) -> Result<()> {
    match action {
        Action::Toggle => {
            send(
                socket,
                Command::Toggle {
                    cleanup: None,
                    style: None,
                    snippet: None,
                },
            )
            .await
        }
        Action::Start => send(socket, Command::Start).await,
        Action::Stop => send(socket, Command::Stop).await,
        Action::Cancel => send(socket, Command::Cancel).await,
        Action::LaunchOverlay => spawn_sibling("skald-overlay", &[]),
        Action::CloseOverlay => spawn("pkill", &["-x", "skald-overlay"]),
        Action::TestMicrophone => spawn_sibling("skald", &["test", "mic"]),
        Action::OpenConfig => {
            let path = Config::path()?;
            spawn("xdg-open", &[path.to_string_lossy().as_ref()])
        }
        Action::OpenDocs => spawn("xdg-open", &["https://tryskald.dev/"]),
        Action::RestartDaemon => spawn("systemctl", &["--user", "restart", "skaldd.service"]),
        Action::Quit => Ok(()),
    }
}

async fn send(socket: &Path, command: Command) -> Result<()> {
    let response = client::request(socket, command).await?;
    if response.ok {
        Ok(())
    } else {
        let message = response
            .error
            .map_or_else(|| "daemon rejected command".into(), |error| error.message);
        bail!("{message}")
    }
}

fn spawn_sibling(binary: &str, args: &[&str]) -> Result<()> {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(binary)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(binary));
    spawn_path(&sibling, args)
}

fn spawn(binary: &str, args: &[&str]) -> Result<()> {
    spawn_path(Path::new(binary), args)
}

fn spawn_path(binary: &Path, args: &[&str]) -> Result<()> {
    ProcessCommand::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch {}", binary.display()))?;
    Ok(())
}
