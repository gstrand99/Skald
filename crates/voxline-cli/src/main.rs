use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Serialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};
use voxline_core::{
    config::Config,
    protocol::{Command, EventKind, PROTOCOL_VERSION, Request, Response},
    runtime::{runtime_dir, socket_path, verify_mode},
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
    Toggle,
    Start,
    Stop,
    Cancel,
    Watch,
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
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
    privacy: PrivacyReport,
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
        Commands::Toggle => print_response(&send(Command::Toggle).await?),
        Commands::Start => print_response(&send(Command::Start).await?),
        Commands::Stop => print_response(&send(Command::Stop).await?),
        Commands::Cancel => print_response(&send(Command::Cancel).await?),
        Commands::Watch => watch().await?,
        Commands::Doctor { json } => doctor(json).await?,
        Commands::Config { command } => config(&command)?,
    }
    Ok(())
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
    let report = DoctorReport {
        environment: voxline_platform::environment_report(),
        config_path: Config::path()?.display().to_string(),
        config_valid,
        runtime_dir: runtime.as_ref().map(|path| path.display().to_string()),
        runtime_secure,
        socket_path: socket.as_ref().map(|path| path.display().to_string()),
        daemon_reachable,
        trigger_mode: "external shortcut",
        recommended_command: "voxline toggle",
        privacy: PrivacyReport {
            cleanup_enabled: config.cleanup.enabled,
            store_audio: config.privacy.store_audio,
            store_raw_transcript: config.privacy.store_raw_transcript,
            store_cleaned_transcript: config.privacy.store_cleaned_transcript,
            log_transcripts: config.privacy.log_transcripts,
        },
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
    println!("Tools:");
    for tool in &report.environment.tools {
        println!("  {:<12} {}", tool.name, yes_no(tool.available));
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
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
