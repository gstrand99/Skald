mod asr;
mod audio;
mod bench;
mod cleanup;
mod delivery;
mod dictation;
mod injection;
mod ipc;
mod jobs;
mod openrouter;
mod preview;
mod preview_asr;
mod template_extract;

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result};
use clap::Parser;
use skald_core::{
    config::{AutoPasteMode, Config},
    protocol::{DaemonStatus, ModelState},
    runtime::{ensure_runtime_dir_for, secure_socket_permissions, socket_path_for},
};
use tokio::{
    net::UnixListener,
    signal,
    sync::{Mutex, RwLock, broadcast},
};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::jobs::AppState;

#[derive(Debug, Parser)]
#[command(version, about = "Skald local dictation daemon")]
struct Args {
    #[arg(long)]
    foreground: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _args = Args::parse();
    let config = Config::load_validated()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&config.daemon.log_level)),
        )
        .init();

    ensure_runtime_dir_for(&config.paths)?;
    let socket = socket_path_for(&config.paths)?;
    remove_stale_socket(&socket)?;
    let listener = UnixListener::bind(&socket)
        .with_context(|| format!("failed to bind {}", socket.display()))?;
    secure_socket_permissions(&socket).context("failed to secure daemon socket permissions")?;
    let (events, _) = broadcast::channel(32);
    let audio_gates = config.audio.gates.clone();
    let paste_available = skald_platform::paste_backend().is_some();
    let auto_paste_effective = match (&config.injection.auto_paste, paste_available) {
        (AutoPasteMode::Off, _) | (_, false) => "clipboard_only",
        (AutoPasteMode::Safe, true) => "safe",
        (AutoPasteMode::Always, true) => "always",
    };
    let preview_enabled = config.preview_enabled_effective();
    let preview_asr = preview_enabled
        .then(|| preview_asr::PreviewAsrManager::spawn(&config.preview, &config.asr));
    let mut preview_config = config.preview.clone();
    preview_config.enabled = preview_enabled;
    let state = Arc::new(AppState {
        status: RwLock::new(DaemonStatus {
            cleanup_enabled: config.cleanup.enabled,
            asr_gpu_build: cfg!(feature = "asr-whisper-rs-cuda"),
            auto_paste_effective: auto_paste_effective.into(),
            preview_model_state: preview_enabled.then_some(ModelState::Unloaded),
            ..DaemonStatus::default()
        }),
        events,
        preview: preview::PreviewCoordinator::new(preview_config),
        preview_asr,
        audio: audio::AudioRecorder::spawn(config.audio, config.paths.clone()),
        asr: asr::AsrManager::spawn(config.asr, config.vocabulary),
        audio_gates,
        injection: config.injection,
        notifications: config.notifications,
        privacy: config.privacy,
        target_at_start: Mutex::new(None),
        cleanup_override: Mutex::new(None),
        style_override: Mutex::new(None),
        active_app_profile: Mutex::new(None),
        job_config: Mutex::new(None),
    });

    info!(path = %socket.display(), "skaldd listening");
    loop {
        tokio::select! {
            incoming = listener.accept() => {
                let (stream, _) = incoming?;
                if ipc::reject_foreign_peer(&stream) {
                    continue;
                }
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(error) = ipc::handle_client(stream, state).await {
                        warn!(%error, "client connection failed");
                    }
                });
            }
            result = signal::ctrl_c() => {
                result?;
                info!("shutdown requested");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(socket);
    Ok(())
}
fn remove_stale_socket(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("failed to remove stale socket {}", path.display()))?;
    }
    Ok(())
}
