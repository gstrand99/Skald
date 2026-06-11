mod cleanup_cmd;
mod secrets_cmd;
mod service;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Serialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};
use voxline_core::{
    cleanup::{CLEANUP_COST_WARNING, CleanupOverride},
    config::Config,
    protocol::{Command, EventKind, PROTOCOL_VERSION, Request, Response, SessionEnvironment},
    runtime::{runtime_dir, socket_path, verify_mode},
    secrets,
};
use voxline_platform::{
    SessionEnvironmentSnapshot, session_environment_mismatch, trigger_guidance,
};

#[derive(Debug, Parser)]
#[command(
    name = "voxline",
    version,
    about = "Control the VoxLine dictation daemon"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Status,
    Toggle {
        #[arg(long)]
        cleanup: bool,
        #[arg(long)]
        no_cleanup: bool,
    },
    Start,
    #[command(name = "ptt-start")]
    PttStart,
    Stop,
    #[command(name = "ptt-stop")]
    PttStop,
    Cancel,
    Watch,
    Transcribe {
        audio_file: std::path::PathBuf,
        #[arg(long)]
        no_cleanup: bool,
    },
    Asr {
        #[command(subcommand)]
        command: AsrCommands,
    },
    Bench {
        #[command(subcommand)]
        command: BenchCommands,
    },
    Vocab {
        #[command(subcommand)]
        command: VocabCommands,
    },
    Record {
        #[arg(long, default_value_t = 5)]
        seconds: u64,
        #[arg(long)]
        no_cleanup: bool,
    },
    Test {
        #[command(subcommand)]
        command: TestCommands,
    },
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
    Secrets {
        #[command(subcommand)]
        command: secrets_cmd::SecretsCommands,
    },
    Cleanup {
        #[command(subcommand)]
        command: CleanupCommands,
    },
}

#[derive(Debug, Subcommand)]
enum CleanupCommands {
    Enable { provider: String },
    Disable,
    Preview { text: String },
}

#[derive(Debug, Subcommand)]
enum ServiceCommands {
    Install,
    Uninstall,
    Start,
    Stop,
    Status,
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    Path,
    Init {
        #[arg(long)]
        force: bool,
    },
    Validate,
}

#[derive(Debug, Subcommand)]
enum TestCommands {
    Mic {
        #[arg(long, default_value_t = 3)]
        seconds: u64,
    },
    Clipboard,
    Paste,
    Openrouter,
}

#[derive(Debug, Subcommand)]
enum AsrCommands {
    Status,
    Load,
    Unload,
    Restart,
}

#[derive(Debug, Subcommand)]
enum BenchCommands {
    Asr { audio_file: std::path::PathBuf },
    ModelLoad,
}

#[derive(Debug, Subcommand)]
enum VocabCommands {
    List,
    Test {
        text: String,
    },
    Add {
        #[command(subcommand)]
        command: VocabAddCommands,
    },
}

#[derive(Debug, Subcommand)]
enum VocabAddCommands {
    Phrase { text: String },
    Replace { from: String, to: String },
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    environment: voxline_platform::EnvironmentReport,
    config_path: String,
    config_valid: bool,
    runtime_dir: Option<String>,
    runtime_secure: bool,
    socket_path: Option<String>,
    daemon_reachable: bool,
    trigger_mode: &'static str,
    recommended_command: &'static str,
    push_to_talk_note: String,
    binding_examples: Vec<String>,
    daemon_environment: Option<SessionEnvironmentSnapshot>,
    environment_mismatch: Option<String>,
    privacy: PrivacyReport,
    asr: AsrReport,
    paste: voxline_platform::PasteReport,
    secrets: secrets::SecretStatus,
    cleanup_provider: String,
    cleanup_warning: Option<String>,
}

#[derive(Debug, Serialize)]
struct AsrReport {
    backend: String,
    model_path: String,
    model_exists: bool,
    gpu_requested: bool,
    lifecycle_mode: String,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct PrivacyReport {
    cleanup_enabled: bool,
    store_audio: bool,
    store_raw_transcript: bool,
    store_cleaned_transcript: bool,
    log_transcripts: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status => print_response(&send(Command::Status).await?),
        Commands::Toggle {
            cleanup,
            no_cleanup,
        } => print_response(
            &send(Command::Toggle {
                cleanup: cleanup_override(cleanup, no_cleanup)?,
            })
            .await?,
        ),
        Commands::Start | Commands::PttStart => print_response(&send(Command::Start).await?),
        Commands::Stop | Commands::PttStop => print_response(&send(Command::Stop).await?),
        Commands::Cancel => print_response(&send(Command::Cancel).await?),
        Commands::Watch => watch().await?,
        Commands::Transcribe {
            audio_file,
            no_cleanup: _,
        } => print_response(
            &send(Command::Transcribe {
                audio_path: audio_file,
            })
            .await?,
        ),
        Commands::Asr { command } => {
            let command = match command {
                AsrCommands::Status => Command::AsrStatus,
                AsrCommands::Load => Command::AsrLoad,
                AsrCommands::Unload => Command::AsrUnload,
                AsrCommands::Restart => Command::AsrRestart,
            };
            print_response(&send(command).await?);
        }
        Commands::Bench { command } => match command {
            BenchCommands::Asr { audio_file } => print_response(
                &send(Command::Transcribe {
                    audio_path: audio_file,
                })
                .await?,
            ),
            BenchCommands::ModelLoad => {
                let _ = send(Command::AsrUnload).await?;
                print_response(&send(Command::AsrLoad).await?);
            }
        },
        Commands::Vocab { command } => vocab(command)?,
        Commands::Record {
            seconds,
            no_cleanup,
        } => record(seconds, cleanup_override(false, no_cleanup)?).await?,
        Commands::Test { command } => match command {
            TestCommands::Mic { seconds } => record(seconds, None).await?,
            TestCommands::Clipboard => print_response(&send(Command::TestClipboard).await?),
            TestCommands::Paste => print_response(&send(Command::TestPaste).await?),
            TestCommands::Openrouter => {
                print_cleanup_response(&send(Command::TestOpenrouter).await?);
            }
        },
        Commands::Doctor { json } => doctor(json).await?,
        Commands::Config { command } => config(&command)?,
        Commands::Service { command } => service_command(&command)?,
        Commands::Secrets { command } => secrets_cmd::run(command)?,
        Commands::Cleanup { command } => match command {
            CleanupCommands::Enable { provider } => cleanup_cmd::enable(&provider)?,
            CleanupCommands::Disable => cleanup_cmd::disable()?,
            CleanupCommands::Preview { text } => {
                print_cleanup_response(&send(Command::CleanupPreview { text }).await?);
            }
        },
    }
    Ok(())
}

fn vocab(command: VocabCommands) -> Result<()> {
    let mut config = Config::load_or_default()?;
    match command {
        VocabCommands::List => {
            for phrase in config.vocabulary.phrases {
                println!("{}", phrase.text);
            }
            for replacement in config.vocabulary.replacements {
                println!("{} -> {}", replacement.from, replacement.to);
            }
        }
        VocabCommands::Test { text } => {
            let mut output = text;
            for replacement in config.vocabulary.replacements {
                output = output.replace(&replacement.from, &replacement.to);
            }
            println!("{output}");
        }
        VocabCommands::Add { command } => {
            match command {
                VocabAddCommands::Phrase { text } => {
                    config
                        .vocabulary
                        .phrases
                        .push(voxline_core::config::VocabularyPhrase { text });
                }
                VocabAddCommands::Replace { from, to } => {
                    config.vocabulary.replacements.push(
                        voxline_core::config::VocabularyReplacement {
                            from,
                            to,
                            case_sensitive: false,
                        },
                    );
                }
            }
            println!("{}", config.save()?.display());
        }
    }
    Ok(())
}

fn cleanup_override(cleanup: bool, no_cleanup: bool) -> Result<Option<CleanupOverride>> {
    if cleanup && no_cleanup {
        bail!("--cleanup and --no-cleanup cannot be used together");
    }
    if cleanup {
        Ok(Some(CleanupOverride::Force))
    } else if no_cleanup {
        Ok(Some(CleanupOverride::Disable))
    } else {
        Ok(None)
    }
}

async fn record(seconds: u64, cleanup: Option<CleanupOverride>) -> Result<()> {
    let started = send(Command::Toggle { cleanup }).await?;
    if !started.ok {
        print_response(&started);
        bail!("recording did not start");
    }
    println!("Recording for {seconds} seconds...");
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    let stopped = send(Command::Toggle { cleanup: None }).await?;
    print_response(&stopped);
    if !stopped.ok {
        bail!("recording did not stop cleanly");
    }
    Ok(())
}

fn service_command(command: &ServiceCommands) -> Result<()> {
    match command {
        ServiceCommands::Install => {
            let config = Config::load_or_default()?;
            service::install(&config.daemon.log_level)
        }
        ServiceCommands::Uninstall => service::uninstall(),
        ServiceCommands::Start => service::start(),
        ServiceCommands::Stop => service::stop(),
        ServiceCommands::Status => service::status(),
    }
}

fn config(command: &ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Path => println!("{}", Config::path()?.display()),
        ConfigCommands::Init { force } => println!("{}", Config::init(*force)?.display()),
        ConfigCommands::Validate => {
            let config = Config::load_or_default()?;
            config.validate()?;
            println!("configuration is valid");
        }
    }
    Ok(())
}

async fn send(command: Command) -> Result<Response> {
    let socket = socket_path()?;
    let stream = UnixStream::connect(&socket).await.with_context(|| {
        format!(
            "cannot connect to {}; is voxlined running?",
            socket.display()
        )
    })?;
    let (reader, mut writer) = stream.into_split();
    let request = Request {
        protocol_version: PROTOCOL_VERSION,
        request_id: ulid::Ulid::new().to_string(),
        command,
    };
    write_request(&mut writer, &request).await?;
    let mut lines = BufReader::new(reader).lines();
    let line = lines
        .next_line()
        .await?
        .context("daemon closed without a response")?;
    Ok(serde_json::from_str(&line)?)
}

async fn watch() -> Result<()> {
    let socket = socket_path()?;
    let stream = UnixStream::connect(&socket).await.with_context(|| {
        format!(
            "cannot connect to {}; is voxlined running?",
            socket.display()
        )
    })?;
    let (reader, mut writer) = stream.into_split();
    let request = Request {
        protocol_version: PROTOCOL_VERSION,
        request_id: ulid::Ulid::new().to_string(),
        command: Command::Subscribe {
            events: vec![EventKind::State, EventKind::Result, EventKind::Error],
        },
    };
    write_request(&mut writer, &request).await?;
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        println!("{line}");
    }
    Ok(())
}

async fn write_request(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &Request,
) -> Result<()> {
    let mut bytes = serde_json::to_vec(request)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    Ok(())
}

fn print_response(response: &Response) {
    println!(
        "{}",
        serde_json::to_string_pretty(&response).expect("response serializes")
    );
}

fn print_cleanup_response(response: &Response) {
    if let Some(text) = &response.cleaned_text {
        println!("{text}");
        if let Some(cleanup_ms) = response.cleanup_ms {
            println!("cleanup_ms: {cleanup_ms}");
        }
        return;
    }
    print_response(response);
}

async fn doctor(json: bool) -> Result<()> {
    let config = Config::load_or_default()?;
    let config_valid = config.validate().is_ok();
    let runtime = runtime_dir().ok();
    let runtime_secure = runtime
        .as_deref()
        .is_some_and(|path| path.exists() && verify_mode(path).is_ok());
    let socket = socket_path().ok();
    let daemon_reachable = match &socket {
        Some(path) => UnixStream::connect(path).await.is_ok(),
        None => false,
    };
    let cli_environment = voxline_platform::environment_report();
    let cli_snapshot = SessionEnvironmentSnapshot::from(&cli_environment);
    let daemon_environment = if daemon_reachable {
        fetch_daemon_environment().await
    } else {
        None
    };
    let environment_mismatch = daemon_environment
        .as_ref()
        .and_then(|daemon| session_environment_mismatch(&cli_snapshot, daemon));
    let session = cli_environment.session_type.as_deref().unwrap_or("unknown");
    let desktop = cli_environment.desktop.as_deref().unwrap_or("unknown");
    let trigger = trigger_guidance(session, desktop);
    let model_path = expand_home(&config.asr.model_path);
    let secret_status = secrets::secret_status(&config.secrets);
    let cleanup_warning = if config.cleanup.enabled {
        Some("Warning: transcript text is sent to the configured cleanup provider.".into())
    } else {
        None
    };
    let report = DoctorReport {
        environment: cli_environment,
        config_path: Config::path()?.display().to_string(),
        config_valid,
        runtime_dir: runtime.as_ref().map(|path| path.display().to_string()),
        runtime_secure,
        socket_path: socket.as_ref().map(|path| path.display().to_string()),
        daemon_reachable,
        trigger_mode: "external shortcut",
        recommended_command: trigger.recommended_command,
        push_to_talk_note: trigger.push_to_talk_note.into(),
        binding_examples: trigger.binding_examples,
        daemon_environment,
        environment_mismatch,
        privacy: PrivacyReport {
            cleanup_enabled: config.cleanup.enabled,
            store_audio: config.privacy.store_audio,
            store_raw_transcript: config.privacy.store_raw_transcript,
            store_cleaned_transcript: config.privacy.store_cleaned_transcript,
            log_transcripts: config.privacy.log_transcripts,
        },
        asr: AsrReport {
            backend: config.asr.backend.clone(),
            model_path: model_path.display().to_string(),
            model_exists: model_path.is_file(),
            gpu_requested: config.asr.gpu,
            lifecycle_mode: config.asr.lifecycle.mode.clone(),
        },
        paste: voxline_platform::paste_report(),
        secrets: secret_status,
        cleanup_provider: config.cleanup.provider.clone(),
        cleanup_warning,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    print_doctor(&report);
    if !report.config_valid {
        bail!("configuration is invalid");
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn print_doctor(report: &DoctorReport) {
    println!("VoxLine doctor");
    println!(
        "Session: {}",
        report
            .environment
            .session_type
            .as_deref()
            .unwrap_or("unknown")
    );
    println!(
        "Desktop: {}",
        report.environment.desktop.as_deref().unwrap_or("unknown")
    );
    println!("Config valid: {}", yes_no(report.config_valid));
    println!("Runtime secure: {}", yes_no(report.runtime_secure));
    println!("Daemon reachable: {}", yes_no(report.daemon_reachable));
    println!("Trigger mode: {}", report.trigger_mode);
    println!("Recommended command: {}", report.recommended_command);
    println!("Push-to-talk: {}", report.push_to_talk_note);
    for line in &report.binding_examples {
        println!("  {line}");
    }
    if let Some(daemon) = &report.daemon_environment {
        println!("Daemon environment:");
        println!(
            "  Session: {}",
            daemon.session_type.as_deref().unwrap_or("unknown")
        );
        println!(
            "  Desktop: {}",
            daemon.desktop.as_deref().unwrap_or("unknown")
        );
        println!(
            "  Wayland display: {}",
            yes_no(daemon.wayland_display_present)
        );
        println!("  DISPLAY: {}", yes_no(daemon.display_present));
        println!(
            "  D-Bus session: {}",
            yes_no(daemon.dbus_session_bus_present)
        );
        println!(
            "  XDG runtime dir: {}",
            yes_no(daemon.xdg_runtime_dir_present)
        );
    }
    if let Some(message) = &report.environment_mismatch {
        println!("Environment warning: {message}");
    }
    println!("Tools:");
    for tool in &report.environment.tools {
        println!("  {:<12} {}", tool.name, yes_no(tool.available));
    }
    println!("Secrets:");
    println!(
        "  Keyring available: {}",
        yes_no(report.secrets.keyring_available)
    );
    println!(
        "  OpenRouter configured: {}",
        yes_no(report.secrets.openrouter_configured)
    );
    println!("  Env fallback: {}", yes_no(report.secrets.env_configured));
    println!("Cleanup:");
    println!("  Provider: {}", report.cleanup_provider);
    println!("  Enabled: {}", yes_no(report.privacy.cleanup_enabled));
    if let Some(message) = &report.cleanup_warning {
        println!("  {message}");
    }
    if report.privacy.cleanup_enabled {
        println!("  Note: {CLEANUP_COST_WARNING}");
    }
    println!("Privacy:");
    println!(
        "  Cleanup enabled: {}",
        yes_no(report.privacy.cleanup_enabled)
    );
    println!("  Store audio: {}", yes_no(report.privacy.store_audio));
    println!(
        "  Log transcripts: {}",
        yes_no(report.privacy.log_transcripts)
    );
    println!("ASR:");
    println!("  Backend: {}", report.asr.backend);
    println!("  Model: {}", report.asr.model_path);
    println!("  Model exists: {}", yes_no(report.asr.model_exists));
    println!("  GPU requested: {}", yes_no(report.asr.gpu_requested));
    println!("  Lifecycle: {}", report.asr.lifecycle_mode);
    println!("Paste:");
    println!("  Backend: {}", report.paste.backend);
    println!(
        "  Clipboard available: {}",
        yes_no(report.paste.clipboard_available)
    );
    println!(
        "  Target detection: {}",
        yes_no(report.paste.target_detection_available)
    );
    println!(
        "  Paste available: {}",
        yes_no(report.paste.paste_available)
    );
    println!("  Behavior: {}", report.paste.reason);
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

async fn fetch_daemon_environment() -> Option<SessionEnvironmentSnapshot> {
    let response = send(Command::DaemonEnvironment).await.ok()?;
    response
        .session_environment
        .map(session_environment_from_protocol)
}

fn session_environment_from_protocol(
    environment: SessionEnvironment,
) -> SessionEnvironmentSnapshot {
    SessionEnvironmentSnapshot {
        session_type: environment.session_type,
        desktop: environment.desktop,
        wayland_display_present: environment.wayland_display_present,
        display_present: environment.display_present,
        dbus_session_bus_present: environment.dbus_session_bus_present,
        xdg_runtime_dir_present: environment.xdg_runtime_dir_present,
    }
}

fn expand_home(path: &str) -> std::path::PathBuf {
    if let Some(relative) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(relative);
    }
    std::path::PathBuf::from(path)
}
