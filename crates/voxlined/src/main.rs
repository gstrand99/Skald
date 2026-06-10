use std::{
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use clap::Parser;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    signal,
    sync::{RwLock, broadcast},
};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use voxline_core::{
    config::Config,
    protocol::{Command, DaemonStatus, Event, PROTOCOL_VERSION, ProtocolError, Request, Response},
    runtime::{ensure_runtime_dir, socket_path},
};

#[derive(Debug, Parser)]
#[command(version, about = "VoxLine local dictation daemon")]
struct Args {
    #[arg(long)]
    foreground: bool,
}

struct AppState {
    status: RwLock<DaemonStatus>,
    events: broadcast::Sender<Event>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _args = Args::parse();
    let config = Config::load_or_default()?;
    config.validate()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&config.daemon.log_level)),
        )
        .init();

    ensure_runtime_dir()?;
    let socket = socket_path()?;
    remove_stale_socket(&socket)?;
    let listener = UnixListener::bind(&socket)
        .with_context(|| format!("failed to bind {}", socket.display()))?;
    let (events, _) = broadcast::channel(32);
    let state = Arc::new(AppState {
        status: RwLock::new(DaemonStatus {
            cleanup_enabled: config.cleanup.enabled,
            ..DaemonStatus::default()
        }),
        events,
    });

    info!(path = %socket.display(), "voxlined listening");
    loop {
        tokio::select! {
            incoming = listener.accept() => {
                let (stream, _) = incoming?;
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(error) = handle_client(stream, state).await {
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

async fn handle_client(stream: UnixStream, state: Arc<AppState>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(error) => {
                let response = Response {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: String::new(),
                    ok: false,
                    status: None,
                    error: Some(ProtocolError {
                        code: "invalid_request".into(),
                        message: error.to_string(),
                    }),
                };
                write_json_line(&mut writer, &response).await?;
                continue;
            }
        };
        if request.protocol_version != PROTOCOL_VERSION {
            let response = error_response(
                request.request_id,
                "protocol_mismatch",
                "client and daemon protocol versions differ",
                None,
            );
            write_json_line(&mut writer, &response).await?;
            continue;
        }
        if let Command::Subscribe { .. } = request.command {
            let response = ok_response(request.request_id, state.status.read().await.clone());
            write_json_line(&mut writer, &response).await?;
            stream_events(&mut writer, state.events.subscribe()).await?;
            return Ok(());
        }
        let response = dispatch(request, &state).await;
        write_json_line(&mut writer, &response).await?;
    }
    Ok(())
}

async fn dispatch(request: Request, state: &AppState) -> Response {
    match request.command {
        Command::Status => ok_response(request.request_id, state.status.read().await.clone()),
        Command::Toggle | Command::Start | Command::Stop | Command::Cancel => {
            let status = state.status.read().await.clone();
            let error = ProtocolError {
                code: "audio_not_implemented".into(),
                message: "audio recording is planned for milestone M2".into(),
            };
            let _ = state.events.send(Event::Error {
                protocol_version: PROTOCOL_VERSION,
                timestamp_ms: now_ms(),
                job_id: status.active_job_id.clone(),
                error: error.clone(),
            });
            error_response(
                request.request_id,
                &error.code,
                &error.message,
                Some(status),
            )
        }
        Command::Subscribe { .. } => unreachable!("subscribe handled before dispatch"),
    }
}

fn ok_response(request_id: String, status: DaemonStatus) -> Response {
    Response {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: true,
        status: Some(status),
        error: None,
    }
}

fn error_response(
    request_id: String,
    code: &str,
    message: &str,
    status: Option<DaemonStatus>,
) -> Response {
    Response {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: false,
        status,
        error: Some(ProtocolError {
            code: code.into(),
            message: message.into(),
        }),
    }
}

async fn write_json_line<T: serde::Serialize>(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    value: &T,
) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    Ok(())
}

async fn stream_events(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    mut receiver: broadcast::Receiver<Event>,
) -> Result<()> {
    loop {
        match receiver.recv().await {
            Ok(event) => write_json_line(writer, &event).await?,
            Err(broadcast::error::RecvError::Lagged(_)) => {
                anyhow::bail!("event subscriber is too slow");
            }
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
