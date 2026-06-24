mod apps_cmd;
mod cleanup_cmd;
mod commands_cmd;
mod models_cmd;
mod secrets_cmd;
mod service;
mod setup_cmd;
mod snippets_cmd;
mod styles_cmd;

use std::{io::Write, time::Duration};

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;
use skald_core::{
    apps, build_info,
    cleanup::{CLEANUP_COST_WARNING, CleanupOverride},
    client, commands,
    config::{AutoPasteMode, Config},
    paths,
    protocol::{Command, Event, EventKind, JobState, ModelState, Response, SessionEnvironment},
    runtime::{runtime_dir_for, socket_path_for, socket_permissions_ok, verify_mode},
    secrets, snippets, styles,
};
use skald_platform::{SessionEnvironmentSnapshot, session_environment_mismatch, trigger_guidance};
use tokio::{io::BufReader, net::UnixStream};

#[derive(Debug, Parser)]
#[command(name = "skald", version, about = "Control the Skald dictation daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Version {
        #[arg(long)]
        json: bool,
    },
    Status,
    Toggle {
        #[arg(long, conflicts_with = "no_cleanup")]
        cleanup: bool,
        #[arg(long, conflicts_with = "cleanup")]
        no_cleanup: bool,
        #[arg(long)]
        style: Option<String>,
        #[arg(long)]
        snippet: Option<String>,
    },
    Start,
    #[command(name = "ptt-start")]
    PttStart,
    Stop,
    #[command(name = "ptt-stop")]
    PttStop,
    Cancel,
    Watch,
    /// Stream privacy-safe JSON updates for a Waybar custom module.
    Waybar,
    Overlay {
        #[command(subcommand)]
        command: Option<OverlayCommands>,
    },
    Transcribe {
        audio_file: std::path::PathBuf,
    },
    Asr {
        #[command(subcommand)]
        command: AsrCommands,
    },
    Models {
        #[arg(long, global = true)]
        json: bool,
        #[command(subcommand)]
        command: models_cmd::ModelsCommands,
    },
    Completions {
        shell: Shell,
    },
    Bench {
        #[command(subcommand)]
        command: BenchCommands,
    },
    Diagnostics {
        #[command(subcommand)]
        command: DiagnosticsCommands,
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
        #[arg(long)]
        include_performance: bool,
    },
    Setup {
        /// Skip setup when a config file already exists (never overwrite).
        #[arg(long)]
        if_missing: bool,
        /// Reconfigure even when a config file already exists.
        #[arg(long)]
        force: bool,
        /// Run without prompts; requires `--force` to overwrite an existing config.
        #[arg(long)]
        non_interactive: bool,
        #[arg(long)]
        json: bool,
        #[command(subcommand)]
        command: Option<SetupSubcommands>,
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
    Styles {
        #[command(subcommand)]
        command: styles_cmd::StylesCommands,
    },
    Apps {
        #[command(subcommand)]
        command: apps_cmd::AppsCommands,
    },
    Snippets {
        #[command(subcommand)]
        command: snippets_cmd::SnippetsCommands,
    },
    #[command(name = "commands")]
    Routing {
        #[command(subcommand)]
        command: commands_cmd::CommandsCommands,
    },
}

#[derive(Debug, Subcommand)]
enum SetupSubcommands {
    /// Record the setup benchmark fixture without running the full wizard.
    Record {
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
}

#[derive(Debug, Subcommand)]
enum OverlayCommands {
    Preview {
        #[arg(long)]
        style: Option<String>,
        #[arg(long)]
        cycle: bool,
        #[arg(long)]
        microphone: bool,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        anchor: Option<String>,
        #[arg(long)]
        save: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CleanupCommands {
    Enable {
        provider: String,
    },
    Disable,
    Preview {
        text: String,
        #[arg(long)]
        style: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceCommands {
    Install,
    Uninstall,
    Start,
    Restart,
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
    /// Migrate, validate, and rewrite config with current defaulted fields.
    Upgrade,
    Profile {
        name: String,
    },
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
    Asr {
        audio_file: std::path::PathBuf,
    },
    EndToEnd {
        audio_file: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
    Dictation {
        audio_file: std::path::PathBuf,
        #[arg(long, conflicts_with = "no_cleanup")]
        cleanup: bool,
        #[arg(long, conflicts_with = "cleanup")]
        no_cleanup: bool,
        #[arg(long)]
        paste: bool,
        #[arg(long)]
        json: bool,
    },
    ModelLoad,
}

#[derive(Debug, Subcommand)]
enum DiagnosticsCommands {
    Performance {
        #[arg(long)]
        json: bool,
    },
    Benchmark {
        audio_file: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
    Clear,
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
#[allow(clippy::struct_excessive_bools)]
struct DoctorReport {
    environment: skald_platform::EnvironmentReport,
    config_path: String,
    config_valid: bool,
    runtime_dir: Option<String>,
    runtime_secure: bool,
    socket_path: Option<String>,
    socket_secure: bool,
    daemon_reachable: bool,
    overlay_session: String,
    trigger_mode: &'static str,
    recommended_command: &'static str,
    push_to_talk_note: String,
    binding_examples: Vec<String>,
    daemon_environment: Option<SessionEnvironmentSnapshot>,
    environment_mismatch: Option<String>,
    privacy: PrivacyReport,
    asr: AsrReport,
    preview: Option<PreviewReport>,
    paste: skald_platform::PasteReport,
    secrets: secrets::SecretStatus,
    cleanup_provider: String,
    cleanup_warning: Option<String>,
    config_layout_ready: bool,
    style_issues: Vec<String>,
    app_issues: Vec<String>,
    snippet_issues: Vec<String>,
    voice_command_conflicts: Vec<String>,
    voice_commands_enabled: bool,
    auto_paste_always: bool,
    audio: AudioReport,
    suggestions: Vec<String>,
    remediation_commands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    performance_warnings: Option<Vec<skald_core::diagnostics::DiagnosticWarning>>,
}

#[derive(Debug, Serialize)]
struct AudioReport {
    input_device_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_device_name: Option<String>,
    supported_input_config: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

#[derive(Debug, Serialize)]
struct AsrReport {
    backend: String,
    model_path: String,
    model_exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_id: Option<String>,
    integrity: String,
    gpu_requested: bool,
    lifecycle_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    daemon_model_state: Option<String>,
}

#[derive(Debug, Serialize)]
struct PreviewReport {
    model_path: String,
    model_exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_id: Option<String>,
    integrity: String,
    gpu_requested: bool,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct PrivacyReport {
    cleanup_enabled: bool,
    store_history: bool,
    store_audio: bool,
    store_raw_transcript: bool,
    store_cleaned_transcript: bool,
    log_transcripts: bool,
    sensitive_options_enabled: bool,
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Version { json } => version(json)?,
        Commands::Status => print_response(&send(Command::Status).await?)?,
        Commands::Toggle {
            cleanup,
            no_cleanup,
            style,
            snippet,
        } => print_response(
            &send(Command::Toggle {
                cleanup: cleanup_override(cleanup, no_cleanup)?,
                style,
                snippet,
            })
            .await?,
        )?,
        Commands::Start | Commands::PttStart => print_response(&send(Command::Start).await?)?,
        Commands::Stop | Commands::PttStop => print_response(&send(Command::Stop).await?)?,
        Commands::Cancel => print_response(&send(Command::Cancel).await?)?,
        Commands::Watch => watch().await?,
        Commands::Waybar => waybar().await?,
        Commands::Overlay { command } => run_overlay(command)?,
        Commands::Transcribe { audio_file } => print_response(
            &send(Command::Transcribe {
                audio_path: audio_file,
            })
            .await?,
        )?,
        Commands::Asr { command } => {
            let command = match command {
                AsrCommands::Status => Command::AsrStatus,
                AsrCommands::Load => Command::AsrLoad,
                AsrCommands::Unload => Command::AsrUnload,
                AsrCommands::Restart => Command::AsrRestart,
            };
            print_response(&send(command).await?)?;
        }
        Commands::Bench { command } => handle_bench(command).await?,
        Commands::Diagnostics { command } => diagnostics(command).await?,
        Commands::Vocab { command } => vocab(command)?,
        Commands::Record {
            seconds,
            no_cleanup,
        } => record(seconds, cleanup_override(false, no_cleanup)?).await?,
        Commands::Test { command } => match command {
            TestCommands::Mic { seconds } => record(seconds, None).await?,
            TestCommands::Clipboard => print_response(&send(Command::TestClipboard).await?)?,
            TestCommands::Paste => print_response(&send(Command::TestPaste).await?)?,
            TestCommands::Openrouter => {
                print_cleanup_response(&send(Command::TestOpenrouter).await?)?;
            }
        },
        Commands::Doctor {
            json,
            include_performance,
        } => doctor(json, include_performance).await?,
        Commands::Setup {
            if_missing,
            force,
            non_interactive,
            json,
            command,
        } => match command {
            Some(SetupSubcommands::Record { seconds }) => setup_cmd::run_record(seconds).await?,
            None => {
                setup_cmd::run(setup_cmd::SetupOptions {
                    if_missing,
                    force,
                    non_interactive,
                    json,
                })
                .await?;
            }
        },
        Commands::Config { command } => config(&command)?,
        Commands::Models { command, json } => models_cmd::run(&command, json).await?,
        Commands::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "skald", &mut std::io::stdout());
        }
        Commands::Service { command } => service_command(&command)?,
        Commands::Secrets { command } => secrets_cmd::run(command)?,
        Commands::Cleanup { command } => match command {
            CleanupCommands::Enable { provider } => cleanup_cmd::enable(&provider)?,
            CleanupCommands::Disable => cleanup_cmd::disable()?,
            CleanupCommands::Preview { text, style } => {
                print_cleanup_response(&send(Command::CleanupPreview { text, style }).await?)?;
            }
        },
        Commands::Styles { command } => styles_cmd::run(command)?,
        Commands::Apps { command } => apps_cmd::run(command)?,
        Commands::Snippets { command } => match command {
            snippets_cmd::SnippetsCommands::Insert { name } => {
                print_response(&send(Command::InsertSnippet { name }).await?)?;
            }
            snippets_cmd::SnippetsCommands::Preview { name, text } => {
                print_cleanup_response(&send(Command::TemplatePreview { name, text }).await?)?;
            }
            _ => snippets_cmd::run(command)?,
        },
        Commands::Routing { command } => commands_cmd::run(command)?,
    }
    Ok(())
}

async fn diagnostics(command: DiagnosticsCommands) -> Result<()> {
    match command {
        DiagnosticsCommands::Performance { json } => {
            print_diagnostics_response(&send(Command::DiagnosticsPerformance).await?, json)?;
        }
        DiagnosticsCommands::Benchmark { audio_file, json } => {
            print_diagnostics_response(
                &send(Command::DiagnosticsBenchmark {
                    audio_path: audio_file,
                })
                .await?,
                json,
            )?;
        }
        DiagnosticsCommands::Clear => {
            let response = send(Command::DiagnosticsClear).await?;
            if response.ok {
                println!("diagnostics records cleared");
            } else {
                print_response(&response)?;
            }
        }
    }
    Ok(())
}

fn version(json: bool) -> Result<()> {
    let info = build_info::build_info("none");
    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("Skald {}", info.version);
        println!("commit: {}", info.commit);
        println!("tag: {}", info.tag);
        println!("target: {}", info.target);
        println!("rustc: {}", info.rustc);
        println!("acceleration: {}", info.acceleration);
        println!("cuda_target: {}", info.cuda_target);
    }
    Ok(())
}

fn vocab(command: VocabCommands) -> Result<()> {
    let mut config = Config::load_validated()?;
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
            let output = skald_core::text::apply_vocabulary_replacements(&text, &config.vocabulary);
            println!("{output}");
        }
        VocabCommands::Add { command } => {
            match command {
                VocabAddCommands::Phrase { text } => {
                    config
                        .vocabulary
                        .phrases
                        .push(skald_core::config::VocabularyPhrase { text });
                }
                VocabAddCommands::Replace { from, to } => {
                    config.vocabulary.replacements.push(
                        skald_core::config::VocabularyReplacement {
                            from,
                            to,
                            case_sensitive: false,
                        },
                    );
                }
            }
            config.validate()?;
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
    let started = send(Command::Toggle {
        cleanup,
        style: None,
        snippet: None,
    })
    .await?;
    if !started.ok {
        print_response(&started)?;
        bail!("recording did not start");
    }
    println!("Recording for {seconds} seconds...");
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    let stopped = send(Command::Toggle {
        cleanup: None,
        style: None,
        snippet: None,
    })
    .await?;
    print_response(&stopped)?;
    if !stopped.ok {
        bail!("recording did not stop cleanly");
    }
    Ok(())
}

fn service_command(command: &ServiceCommands) -> Result<()> {
    match command {
        ServiceCommands::Install => {
            let config = Config::load_validated()?;
            service::install(&config.daemon.log_level)
        }
        ServiceCommands::Uninstall => service::uninstall(),
        ServiceCommands::Start => service::start(),
        ServiceCommands::Restart => service::restart(),
        ServiceCommands::Stop => service::stop(),
        ServiceCommands::Status => service::status(),
    }
}

fn config(command: &ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Path => println!("{}", Config::path()?.display()),
        ConfigCommands::Init { force } => println!("{}", Config::init(*force)?.display()),
        ConfigCommands::Validate => {
            Config::load_validated()?;
            println!("configuration is valid");
        }
        ConfigCommands::Upgrade => {
            let path = Config::upgrade()?;
            println!("Upgraded configuration at {}", path.display());
        }
        ConfigCommands::Profile { name } => {
            let mut config = Config::load_or_default()?;
            config.apply_profile(name)?;
            let path = config.save()?;
            println!("Applied config profile {name} in {}", path.display());
            println!("Restart skaldd if it is already running.");
        }
    }
    Ok(())
}

pub(crate) async fn send(command: Command) -> Result<Response> {
    let socket = configured_socket_path()?;
    client::request(&socket, command).await
}

async fn watch() -> Result<()> {
    let socket = configured_socket_path()?;
    let (response, reader) = client::subscribe(
        &socket,
        vec![
            EventKind::State,
            EventKind::Result,
            EventKind::Error,
            EventKind::Preview,
        ],
    )
    .await?;
    if !response.ok {
        if let Some(error) = &response.error {
            bail!("{} ({})", error.message, error.code);
        }
        bail!("subscribe rejected");
    }
    let mut reader = BufReader::new(reader);
    let mut preview_display = PreviewDisplay::default();
    let mut recording = false;
    loop {
        let event = match client::read_event(&mut reader).await {
            Ok(event) => event,
            Err(error) if error.to_string() == "daemon closed the event stream" => break,
            Err(error) => return Err(error),
        };
        match event {
            Event::Preview {
                stable,
                provisional,
                speech_active,
                ..
            } => {
                preview_display.update(&stable, &provisional, speech_active);
            }
            Event::AudioLevel { .. } => {}
            Event::State { ref job_state, .. } => match job_state {
                JobState::Recording => {
                    recording = true;
                    preview_display.begin();
                }
                JobState::Stopping => {
                    recording = false;
                    preview_display.finish();
                    println!("transcribing…");
                }
                JobState::Transcribing | JobState::Cleaning => {
                    recording = false;
                }
                JobState::Idle | JobState::Done | JobState::Cancelled | JobState::Failed { .. } => {
                    recording = false;
                    preview_display.finish();
                }
                _ if recording => {}
                _ => {
                    let line = serde_json::to_string(&event)?;
                    println!("{line}");
                }
            },
            Event::Result { result, .. } => {
                recording = false;
                preview_display.finish();
                println!(
                    "result: job={} total_ms={} copied={} paste_attempted={} paste_succeeded={} clipboard_restored={} cleanup_used={} cleanup_failed={}",
                    result.job_id.0,
                    result.total_ms,
                    result.copied_to_clipboard,
                    result.paste_attempted,
                    result.paste_succeeded,
                    result.clipboard_restored,
                    result.cleanup_used,
                    result.cleanup_failed,
                );
                if let Some(snippet) = &result.snippet_used {
                    println!("snippet: {snippet}");
                }
                println!("insertion: {}", result.insertion_reason);
                if let Some(transcript) = &result.transcript {
                    println!("final: {}", transcript.text);
                }
            }
            Event::Error { error, .. } => {
                recording = false;
                preview_display.finish();
                println!("error: {} ({})", error.message, error.code);
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct PreviewDisplay {
    stable: String,
    provisional: String,
    active: bool,
}

impl PreviewDisplay {
    fn begin(&mut self) {
        self.stable.clear();
        self.provisional.clear();
        self.active = true;
        self.redraw(true);
    }

    fn update(&mut self, stable: &str, provisional: &str, speech_active: bool) {
        if !self.active {
            self.begin();
        }
        let changed = self.stable != stable || self.provisional != provisional;
        stable.clone_into(&mut self.stable);
        provisional.clone_into(&mut self.provisional);
        if changed || (speech_active && self.stable.is_empty() && self.provisional.is_empty()) {
            self.redraw(speech_active);
        }
    }

    fn finish(&mut self) {
        if self.active {
            println!();
            self.active = false;
            self.stable.clear();
            self.provisional.clear();
        }
    }

    fn redraw(&self, speech_active: bool) {
        let mut line = String::from("\r\x1b[2Kpreview: ");
        if self.stable.is_empty() && self.provisional.is_empty() {
            line.push_str(if speech_active { "…" } else { "(quiet)" });
        } else {
            line.push_str(&self.stable);
            if !self.provisional.is_empty() {
                if !self.stable.is_empty() {
                    line.push(' ');
                }
                line.push_str("\x1b[2m");
                line.push_str(&self.provisional);
                line.push_str("\x1b[0m");
            }
        }
        print!("{line}");
        std::io::stdout().flush().ok();
    }
}

fn run_overlay(command: Option<OverlayCommands>) -> Result<()> {
    let overlay = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|dir| dir.join("skald-overlay"))
                .filter(|candidate| candidate.is_file())
        })
        .unwrap_or_else(|| std::path::PathBuf::from("skald-overlay"));
    let mut process = std::process::Command::new(overlay);
    if let Some(OverlayCommands::Preview {
        style,
        cycle,
        microphone,
        mode,
        anchor,
        save,
    }) = command
    {
        process.arg("--preview");
        if let Some(style) = style {
            process.args(["--style", &style]);
        }
        if cycle {
            process.arg("--cycle");
        }
        if microphone {
            process.arg("--microphone");
        }
        if let Some(mode) = mode {
            process.args(["--mode", &mode]);
        }
        if let Some(anchor) = anchor {
            process.args(["--anchor", &anchor]);
        }
        if save {
            process.arg("--save");
        }
    }
    let status = process.status().context("failed to launch skald-overlay")?;
    if status.success() {
        Ok(())
    } else {
        bail!("skald-overlay exited with {status}");
    }
}

async fn waybar() -> Result<()> {
    let socket = client::socket_path_from_config()?;
    let kinds = vec![EventKind::State, EventKind::Result, EventKind::Error];
    let mut backoff = Duration::from_secs(1);
    let mut last_json = String::new();

    emit_waybar_status(
        &skald_core::desktop::DesktopStatus::disconnected(),
        &mut last_json,
    )?;

    loop {
        match client::request(&socket, Command::Status).await {
            Ok(response) if response.ok => {
                if let Some(status) = response.status {
                    emit_waybar_status(
                        &skald_core::desktop::DesktopStatus::from_daemon(&status),
                        &mut last_json,
                    )?;
                }
            }
            _ => {
                emit_waybar_status(
                    &skald_core::desktop::DesktopStatus::disconnected(),
                    &mut last_json,
                )?;
            }
        }

        match client::subscribe(&socket, kinds.clone()).await {
            Ok((response, reader)) if response.ok => {
                backoff = Duration::from_secs(1);
                let mut reader = BufReader::new(reader);
                while let Ok(event) = client::read_event(&mut reader).await {
                    if let Some(status) = skald_core::desktop::DesktopStatus::from_event(&event) {
                        emit_waybar_status(&status, &mut last_json)?;
                    }
                }
            }
            _ => {}
        }

        emit_waybar_status(
            &skald_core::desktop::DesktopStatus::disconnected(),
            &mut last_json,
        )?;
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(15));
    }
}

fn emit_waybar_status(
    status: &skald_core::desktop::DesktopStatus,
    last_json: &mut String,
) -> Result<()> {
    let json = serde_json::to_string(status).context("failed to serialize Waybar status")?;
    if json != *last_json {
        println!("{json}");
        std::io::stdout()
            .flush()
            .context("failed to flush Waybar status")?;
        json.clone_into(last_json);
    }
    Ok(())
}

pub(crate) fn print_response(response: &Response) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(response).context("failed to serialize daemon response")?
    );
    Ok(())
}

fn print_cleanup_response(response: &Response) -> Result<()> {
    if let Some(text) = &response.cleaned_text {
        println!("{text}");
        if let Some(cleanup_ms) = response.cleanup_ms {
            println!("cleanup_ms: {cleanup_ms}");
        }
        return Ok(());
    }
    print_response(response)
}

fn print_diagnostics_response(response: &Response, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(response)?);
        return Ok(());
    }
    if !response.ok {
        return print_response(response);
    }
    let Some(snapshot) = &response.diagnostics else {
        println!("no diagnostics returned");
        return Ok(());
    };
    println!(
        "diagnostics: enabled={} retained={}/{} dropped={}",
        snapshot.enabled, snapshot.records_retained, snapshot.capacity, snapshot.dropped_records
    );
    if !snapshot.enabled {
        println!("performance diagnostics are disabled; set diagnostics.enabled = true");
        return Ok(());
    }
    if snapshot.records.is_empty() {
        println!("no performance records retained");
    }
    for warning in &snapshot.warnings {
        println!("warning: {}: {}", warning.code, warning.message);
    }
    for record in &snapshot.records {
        println!(
            "#{} {} {} model={} backend={} threads={}",
            record.sequence,
            record.source_name(),
            record.outcome.status,
            record.context.model,
            record.context.acceleration_backend,
            record.context.thread_count
        );
        println!(
            "  recording={} asr={} rtf={} load={} total={}",
            format_measurement_ms(&record.timings.recording_duration_ms),
            format_measurement_ms(&record.timings.asr_inference_ms),
            format_rtf(&record.timings.asr_real_time_factor_milli),
            format_measurement_ms(&record.timings.model_load_ms),
            format_measurement_ms(&record.timings.end_to_end_ms)
        );
        println!(
            "  cleanup={} insertion={} clipboard={} paste={}",
            format_measurement_ms(&record.cleanup.duration_ms),
            record.insertion.outcome,
            format_measurement_ms(&record.timings.clipboard_ms),
            format_measurement_ms(&record.timings.paste_attempt_ms)
        );
        if let Some(code) = &record.outcome.error_code {
            println!("  error={code}");
        }
        if let Some(code) = &record.insertion.warning_code {
            println!("  insertion_warning={code}");
        }
    }
    Ok(())
}

trait DiagnosticSourceName {
    fn source_name(&self) -> &'static str;
}

impl DiagnosticSourceName for skald_core::diagnostics::PerformanceRecord {
    fn source_name(&self) -> &'static str {
        match &self.source {
            skald_core::diagnostics::DiagnosticSource::Dictation => "dictation",
            skald_core::diagnostics::DiagnosticSource::Benchmark => "benchmark",
        }
    }
}

fn format_measurement_ms(value: &skald_core::diagnostics::Measurement<u64>) -> String {
    match value {
        skald_core::diagnostics::Measurement::Unavailable => "unavailable".into(),
        skald_core::diagnostics::Measurement::NotAttempted => "not_attempted".into(),
        skald_core::diagnostics::Measurement::Failed { code } => format!("failed({code})"),
        skald_core::diagnostics::Measurement::Value(ms) => format!("{ms}ms"),
    }
}

fn format_rtf(value: &skald_core::diagnostics::Measurement<u64>) -> String {
    match value {
        skald_core::diagnostics::Measurement::Value(milli) => {
            format!("{}.{:02}x", milli / 1000, (milli % 1000) / 10)
        }
        other => format_measurement_ms(other),
    }
}

async fn model_integrity(
    model_dir: &std::path::Path,
    model_path: &std::path::Path,
) -> (Option<String>, String) {
    let Some(entry) = skald_core::models::catalog_entry_for_path(model_dir, model_path) else {
        return (
            None,
            if model_path.is_file() {
                "unverified user-managed path".into()
            } else {
                "missing unverified path".into()
            },
        );
    };
    if !model_path.is_file() {
        return (Some(entry.id.into()), "missing".into());
    }
    let integrity = match skald_core::download::verify_model_file(
        model_path,
        entry.expected_size,
        entry.sha256,
    )
    .await
    {
        Ok(()) => "verified".into(),
        Err(error) => format!("invalid: {error}"),
    };
    (Some(entry.id.into()), integrity)
}

async fn preview_doctor_report(
    config: &Config,
    model_dir: &std::path::Path,
) -> Option<PreviewReport> {
    if config.preview_enabled_effective() {
        let preview_model_path = paths::expand_home(&config.preview.effective_model_path());
        let (catalog_id, integrity) = model_integrity(model_dir, &preview_model_path).await;
        Some(PreviewReport {
            model_path: preview_model_path.display().to_string(),
            model_exists: preview_model_path.is_file(),
            catalog_id,
            integrity,
            gpu_requested: config.preview.gpu,
        })
    } else {
        None
    }
}

async fn doctor(json: bool, include_performance: bool) -> Result<()> {
    let config = Config::load_or_default()?;
    let mut report = build_doctor_report(&config).await?;
    if include_performance {
        report.performance_warnings = fetch_performance_warnings().await?;
    }
    report.suggestions = build_doctor_suggestions(&report);
    report.remediation_commands = build_doctor_remediation(&report);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        if doctor_has_failures(&report) {
            bail!("doctor found configuration or socket issues");
        }
        return Ok(());
    }
    print_doctor(&report);
    if !report.config_valid {
        bail!("configuration is invalid");
    }
    Ok(())
}

fn doctor_has_failures(report: &DoctorReport) -> bool {
    !report.config_valid
        || report
            .socket_path
            .as_ref()
            .is_some_and(|_| !report.socket_secure)
}

async fn fetch_performance_warnings()
-> Result<Option<Vec<skald_core::diagnostics::DiagnosticWarning>>> {
    match send(Command::DiagnosticsPerformance).await {
        Ok(response) if response.ok => Ok(response.diagnostics.map(|snapshot| snapshot.warnings)),
        Ok(_) | Err(_) => Ok(None),
    }
}

#[allow(clippy::too_many_lines)]
async fn build_doctor_report(config: &Config) -> Result<DoctorReport> {
    let config_valid = config.validate().is_ok();
    let runtime = runtime_dir_for(&config.paths).ok();
    let runtime_secure = runtime
        .as_deref()
        .is_some_and(|path| path.exists() && verify_mode(path).is_ok());
    let socket = socket_path_for(&config.paths).ok();
    let socket_secure = socket
        .as_ref()
        .is_some_and(|path| path.exists() && socket_permissions_ok(path));
    let daemon_reachable = match &socket {
        Some(path) => UnixStream::connect(path).await.is_ok(),
        None => false,
    };
    let overlay_hint = skald_platform::overlay_session_hint();
    let cli_environment = skald_platform::environment_report();
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
    let model_path = paths::expand_home(&config.asr.model_path);
    let model_dir = paths::resolve_model_dir(&config.paths);
    let (catalog_id, integrity) = model_integrity(&model_dir, &model_path).await;
    let preview = preview_doctor_report(config, &model_dir).await;
    let audio = probe_audio_input();
    let daemon_model_state = if daemon_reachable {
        fetch_daemon_asr_state().await
    } else {
        None
    };
    let secret_status = secrets::secret_status(&config.secrets);
    let cleanup_warning = if config.cleanup.enabled {
        Some("Warning: transcript text is sent to the configured cleanup provider.".into())
    } else {
        None
    };
    Ok(DoctorReport {
        environment: cli_environment,
        config_path: Config::path()?.display().to_string(),
        config_valid,
        runtime_dir: runtime.as_ref().map(|path| path.display().to_string()),
        runtime_secure,
        socket_path: socket.as_ref().map(|path| path.display().to_string()),
        socket_secure,
        daemon_reachable,
        overlay_session: overlay_hint.id.into(),
        trigger_mode: "external shortcut",
        recommended_command: trigger.recommended_command,
        push_to_talk_note: trigger.push_to_talk_note.into(),
        binding_examples: trigger.binding_examples,
        daemon_environment,
        environment_mismatch,
        privacy: PrivacyReport {
            cleanup_enabled: config.cleanup.enabled,
            store_history: config.privacy.store_history,
            store_audio: config.privacy.store_audio,
            store_raw_transcript: config.privacy.store_raw_transcript,
            store_cleaned_transcript: config.privacy.store_cleaned_transcript,
            log_transcripts: config.privacy.log_transcripts,
            sensitive_options_enabled: config.privacy.sensitive_storage_or_logging_enabled(),
        },
        asr: AsrReport {
            backend: config.asr.backend.clone(),
            model_path: model_path.display().to_string(),
            model_exists: model_path.is_file(),
            catalog_id,
            integrity,
            gpu_requested: config.asr.gpu,
            lifecycle_mode: config.asr.lifecycle.mode.clone(),
            daemon_model_state,
        },
        preview,
        paste: skald_platform::paste_report(),
        secrets: secret_status,
        cleanup_provider: config.cleanup.provider.clone(),
        cleanup_warning,
        config_layout_ready: paths::layout_is_scaffolded(&config.paths),
        style_issues: styles::validate_installed_styles(&config.paths)
            .into_iter()
            .map(|issue| format!("{}: {}", issue.style, issue.message))
            .collect(),
        app_issues: apps::validate_installed_app_profiles(&config.paths)
            .into_iter()
            .map(|issue| format!("{}: {}", issue.app, issue.message))
            .collect(),
        snippet_issues: snippets::validate_installed_snippets(&config.paths)
            .into_iter()
            .map(|issue| format!("{}: {}", issue.snippet, issue.message))
            .collect(),
        voice_command_conflicts: commands::build_command_registry(&config.paths)
            .map(|registry| {
                commands::detect_command_conflicts(&registry)
                    .into_iter()
                    .map(|issue| format!("{}: {}", issue.alias, issue.targets.join(", ")))
                    .collect()
            })
            .unwrap_or_default(),
        voice_commands_enabled: config.voice_commands.enabled,
        auto_paste_always: config.injection.auto_paste == AutoPasteMode::Always,
        audio,
        suggestions: Vec::new(),
        remediation_commands: Vec::new(),
        performance_warnings: None,
    })
}

fn probe_audio_input() -> AudioReport {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        return AudioReport {
            input_device_present: false,
            input_device_name: None,
            supported_input_config: false,
            warning: Some("no default input device".into()),
        };
    };
    let name = device.name().ok();
    match device.default_input_config() {
        Ok(_) => AudioReport {
            input_device_present: true,
            input_device_name: name,
            supported_input_config: true,
            warning: None,
        },
        Err(error) => AudioReport {
            input_device_present: true,
            input_device_name: name,
            supported_input_config: false,
            warning: Some(format!("no supported input config: {error}")),
        },
    }
}

async fn fetch_daemon_asr_state() -> Option<String> {
    let response = send(Command::AsrStatus).await.ok()?;
    if !response.ok {
        return None;
    }
    response
        .status
        .map(|status| format_model_state(&status.final_model_state))
}

fn format_model_state(state: &ModelState) -> String {
    match state {
        ModelState::Unloaded => "unloaded".into(),
        ModelState::Loading => "loading".into(),
        ModelState::Ready => "ready".into(),
        ModelState::Failed { code, message } => format!("failed ({code}: {message})"),
    }
}

#[allow(clippy::too_many_lines)]
fn print_doctor(report: &DoctorReport) {
    println!("Skald doctor");
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
    println!("CLI environment:");
    println!(
        "  Wayland display: {}",
        yes_no(report.environment.wayland_display_present)
    );
    println!("  DISPLAY: {}", yes_no(report.environment.display_present));
    println!(
        "  D-Bus session: {}",
        yes_no(report.environment.dbus_session_bus_present)
    );
    println!("Config valid: {}", yes_no(report.config_valid));
    println!("Runtime secure: {}", yes_no(report.runtime_secure));
    println!("Socket secure: {}", yes_no(report.socket_secure));
    println!("Daemon reachable: {}", yes_no(report.daemon_reachable));
    println!("Overlay session: {}", report.overlay_session);
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
    println!(
        "  Insecure file fallback: {}",
        yes_no(report.secrets.insecure_file_enabled)
    );
    if report.secrets.insecure_file_enabled {
        println!("  Warning: plaintext secrets file fallback is enabled.");
    }
    println!("Audio:");
    println!(
        "  Input device: {}",
        yes_no(report.audio.input_device_present)
    );
    if let Some(name) = &report.audio.input_device_name {
        println!("  Device name: {name}");
    }
    println!(
        "  Supported input config: {}",
        yes_no(report.audio.supported_input_config)
    );
    if let Some(warning) = &report.audio.warning {
        println!("  Warning: {warning}");
    }
    println!("Config layout:");
    println!(
        "  styles/apps/snippets dirs: {}",
        yes_no(report.config_layout_ready)
    );
    if report.style_issues.is_empty() {
        println!("  cleanup styles: valid");
    } else {
        for issue in &report.style_issues {
            println!("  style issue: {issue}");
        }
    }
    if report.app_issues.is_empty() {
        println!("  app profiles: valid");
    } else {
        for issue in &report.app_issues {
            println!("  app profile issue: {issue}");
        }
    }
    if report.snippet_issues.is_empty() {
        println!("  insert snippets: valid");
    } else {
        for issue in &report.snippet_issues {
            println!("  snippet issue: {issue}");
        }
    }
    println!(
        "  voice commands: {} (experimental)",
        if report.voice_commands_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    if report.voice_command_conflicts.is_empty() {
        println!("  voice command aliases: no conflicts");
    } else {
        for issue in &report.voice_command_conflicts {
            println!("  voice command conflict: {issue}");
        }
    }
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
    println!("  Store history: {}", yes_no(report.privacy.store_history));
    println!("  Store audio: {}", yes_no(report.privacy.store_audio));
    println!(
        "  Store raw transcript: {}",
        yes_no(report.privacy.store_raw_transcript)
    );
    println!(
        "  Store cleaned transcript: {}",
        yes_no(report.privacy.store_cleaned_transcript)
    );
    println!(
        "  Log transcripts: {}",
        yes_no(report.privacy.log_transcripts)
    );
    if report.privacy.sensitive_options_enabled {
        println!("  Warning: a sensitive [privacy] option is enabled.");
    }
    println!("ASR:");
    println!("  Backend: {}", report.asr.backend);
    println!("  Model: {}", report.asr.model_path);
    println!("  Model exists: {}", yes_no(report.asr.model_exists));
    if let Some(id) = &report.asr.catalog_id {
        println!("  Catalog ID: {id}");
    }
    println!("  Integrity: {}", report.asr.integrity);
    println!("  GPU requested: {}", yes_no(report.asr.gpu_requested));
    println!("  Lifecycle: {}", report.asr.lifecycle_mode);
    if let Some(state) = &report.asr.daemon_model_state {
        println!("  Daemon model state: {state}");
    }
    if let Some(preview) = &report.preview {
        println!("Preview:");
        println!("  Model: {}", preview.model_path);
        println!("  Model exists: {}", yes_no(preview.model_exists));
        if let Some(id) = &preview.catalog_id {
            println!("  Catalog ID: {id}");
        }
        println!("  Integrity: {}", preview.integrity);
        println!("  GPU requested: {}", yes_no(preview.gpu_requested));
    }
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
    if report.auto_paste_always {
        println!("  Warning: auto_paste = \"always\" bypasses paste-safety target checks.");
    }
    println!("  Behavior: {}", report.paste.reason);
    if let Some(warnings) = &report.performance_warnings {
        println!("Performance:");
        if warnings.is_empty() {
            println!("  no warnings");
        } else {
            for warning in warnings {
                println!("  {}: {}", warning.code, warning.message);
            }
        }
    }
    if !report.suggestions.is_empty() {
        println!("Suggestions:");
        for suggestion in &report.suggestions {
            println!("  - {suggestion}");
        }
    }
    if !report.remediation_commands.is_empty() {
        println!("Recommended remediation:");
        for command in &report.remediation_commands {
            println!("  {command}");
        }
    }
}

fn build_doctor_suggestions(report: &DoctorReport) -> Vec<String> {
    let mut suggestions = Vec::new();
    let config = Config::load_or_default().ok();
    if config
        .as_ref()
        .is_none_or(|config| skald_core::setup::needs_setup(&config.paths))
    {
        suggestions.push("Run `skald setup` to complete first-time installation.".into());
    }
    if !report.config_valid {
        suggestions.push("Run `skald config validate` and fix the reported errors.".into());
    }
    if !report.runtime_secure {
        suggestions.push(
            "Ensure the runtime directory exists with mode 0700 and is owned by your user.".into(),
        );
    }
    if report.socket_path.is_some() && !report.socket_secure {
        suggestions.push("Restart skaldd so it recreates the daemon socket with mode 0600.".into());
    }
    if !report.daemon_reachable {
        suggestions
            .push("Start the daemon with `skald service start` or `skaldd --foreground`.".into());
    }
    if !report.asr.model_exists {
        suggestions
            .push("Run `skald models list`, then install and select a catalog model.".into());
    } else if report.asr.integrity != "verified" && report.asr.catalog_id.is_some() {
        let id = report.asr.catalog_id.as_deref().unwrap_or_default();
        suggestions.push(format!(
            "Run `skald models verify {id}`; remove the invalid file before reinstalling it."
        ));
    }
    if let Some(preview) = &report.preview
        && !preview.model_exists
    {
        suggestions.push(
            "Run `skald models install small.en` and `skald models select-preview small.en`, or disable text preview.".into(),
        );
    }
    if report.environment_mismatch.is_some() {
        suggestions.push(
            "Import your graphical session into systemd using the binding examples above.".into(),
        );
    }
    if report.privacy.cleanup_enabled {
        suggestions.push(
            "Cleanup is enabled; transcript text is sent to your configured cloud provider.".into(),
        );
    }
    if report.privacy.sensitive_options_enabled {
        suggestions.push(
            "Review [privacy] in config.toml; storage or transcript logging is enabled.".into(),
        );
    }
    if report.cleanup_provider == "openrouter"
        && report.privacy.cleanup_enabled
        && !report.secrets.openrouter_configured
    {
        suggestions.push("Run `skald secrets set openrouter` before using cleanup.".into());
    }
    if report.secrets.insecure_file_enabled {
        suggestions.push(
            "Migrate to the keyring with `skald secrets set openrouter` and disable allow_insecure_file_fallback in config.".into(),
        );
    }
    if report.auto_paste_always {
        suggestions.push(
            "auto_paste = \"always\" bypasses paste-safety target checks; consider \"safe\"."
                .into(),
        );
    }
    suggestions
}

fn build_doctor_remediation(report: &DoctorReport) -> Vec<String> {
    let mut commands = Vec::new();
    if !report.asr.model_exists {
        commands.push("skald models recommend".into());
        commands.push("skald models install small.en --select".into());
    } else if let Some(id) = &report.asr.catalog_id
        && report.asr.integrity != "verified"
    {
        commands.push(format!("skald models verify {id}"));
    }
    if report
        .preview
        .as_ref()
        .is_some_and(|preview| !preview.model_exists)
    {
        commands.push("skald models install small.en --select-preview".into());
    }
    if !report.daemon_reachable {
        commands.push("skald service start".into());
    }
    commands
}

async fn handle_bench(command: BenchCommands) -> Result<()> {
    match command {
        BenchCommands::Asr { audio_file } => print_response(
            &send(Command::Transcribe {
                audio_path: audio_file,
            })
            .await?,
        )?,
        BenchCommands::EndToEnd { audio_file, json } => {
            bench_transcribe(
                &send(Command::Transcribe {
                    audio_path: audio_file,
                })
                .await?,
                json,
            )?;
        }
        BenchCommands::Dictation {
            audio_file,
            cleanup,
            no_cleanup,
            paste,
            json,
        } => {
            let cleanup_override = cleanup_override(cleanup, no_cleanup)?;
            bench_dictation(
                &send(Command::BenchDictation {
                    audio_path: audio_file,
                    cleanup: cleanup_override,
                    attempt_paste: paste,
                })
                .await?,
                json,
            )?;
        }
        BenchCommands::ModelLoad => {
            let _ = send(Command::AsrUnload).await?;
            print_response(&send(Command::AsrLoad).await?)?;
        }
    }
    Ok(())
}

fn bench_transcribe(response: &Response, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(response)?);
        return Ok(());
    }
    if !response.ok {
        print_response(response)?;
        bail!("benchmark failed");
    }
    let Some(benchmark) = &response.benchmark else {
        bail!("daemon response did not include benchmark timings");
    };
    println!("Skald benchmark (transcribe only)");
    print_configured_model_id();
    print_asr_benchmark(benchmark, response.cleanup_ms);
    Ok(())
}

fn bench_dictation(response: &Response, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(response)?);
        return Ok(());
    }
    if !response.ok {
        print_response(response)?;
        bail!("benchmark failed");
    }
    let Some(dictation) = &response.dictation else {
        bail!("daemon response did not include dictation benchmark data");
    };
    println!("Skald benchmark (full dictation path)");
    print_configured_model_id();
    print_asr_benchmark(&dictation.benchmark, response.cleanup_ms);
    if dictation.paste_succeeded {
        println!("  Stop-to-insert:    {} ms", dictation.total_ms);
    } else {
        println!("  Stop-to-clipboard: {} ms", dictation.total_ms);
    }
    println!(
        "  Clipboard copied:  {}",
        yes_no(dictation.copied_to_clipboard)
    );
    println!("  Paste attempted:   {}", yes_no(dictation.paste_attempted));
    println!("  Paste succeeded:   {}", yes_no(dictation.paste_succeeded));
    println!("  Cleanup used:      {}", yes_no(dictation.cleanup_used));
    println!("  Insertion:         {}", dictation.insertion_reason);
    Ok(())
}

fn print_configured_model_id() {
    let Ok(config) = Config::load_or_default() else {
        return;
    };
    let model_dir = paths::resolve_model_dir(&config.paths);
    let model_path = paths::expand_home(&config.asr.model_path);
    if let Some(entry) = skald_core::models::catalog_entry_for_path(&model_dir, &model_path) {
        println!("  Catalog model:  {}", entry.id);
    } else {
        println!("  Catalog model:  unverified custom path");
    }
}

fn print_asr_benchmark(benchmark: &skald_core::protocol::AsrBenchmark, cleanup_ms: Option<u64>) {
    println!("  Audio duration: {} ms", benchmark.audio_duration_ms);
    println!("  Model load:     {} ms", benchmark.model_load_ms);
    println!("  Transcribe:     {} ms", benchmark.transcribe_ms);
    println!(
        "  Total ASR:      {} ms",
        benchmark.model_load_ms + benchmark.transcribe_ms
    );
    if let Some(cleanup_ms) = cleanup_ms {
        println!("  Cleanup:        {cleanup_ms} ms");
    }
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

fn configured_socket_path() -> Result<std::path::PathBuf> {
    let config = Config::load_validated()?;
    socket_path_for(&config.paths).map_err(Into::into)
}
