mod asr;
mod audio;

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
    protocol::{
        AsrBenchmark, AudioRecording, Command, DaemonStatus, Event, JobId, JobState, ModelState,
        PROTOCOL_VERSION, ProtocolError, Request, Response, Transcript,
    },
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
    audio: audio::AudioRecorder,
    asr: asr::AsrManager,
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
            asr_gpu_build: cfg!(feature = "asr-whisper-rs-cuda"),
            ..DaemonStatus::default()
        }),
        events,
        audio: audio::AudioRecorder::spawn(config.audio),
        asr: asr::AsrManager::spawn(config.asr, config.vocabulary),
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
                    recording: None,
                    transcript: None,
                    benchmark: None,
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
        Command::Status | Command::AsrStatus => {
            ok_response(request.request_id, state.status.read().await.clone())
        }
        Command::Toggle => toggle(request.request_id, state).await,
        Command::Start => start(request.request_id, state).await,
        Command::Stop => stop(request.request_id, state).await,
        Command::Cancel => cancel(request.request_id, state).await,
        Command::Transcribe { audio_path } => {
            transcribe(request.request_id, state, audio_path).await
        }
        Command::AsrLoad => asr_load(request.request_id, state).await,
        Command::AsrUnload => asr_unload(request.request_id, state).await,
        Command::AsrRestart => asr_restart(request.request_id, state).await,
        Command::Subscribe { .. } => unreachable!("subscribe handled before dispatch"),
    }
}

fn ok_response(request_id: String, status: DaemonStatus) -> Response {
    success_response(request_id, status, None)
}

fn success_response(
    request_id: String,
    status: DaemonStatus,
    recording: Option<AudioRecording>,
) -> Response {
    data_response(request_id, status, recording, None, None)
}

fn data_response(
    request_id: String,
    status: DaemonStatus,
    recording: Option<AudioRecording>,
    transcript: Option<Transcript>,
    benchmark: Option<AsrBenchmark>,
) -> Response {
    Response {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: true,
        status: Some(status),
        recording,
        transcript,
        benchmark,
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
        recording: None,
        transcript: None,
        benchmark: None,
        error: Some(ProtocolError {
            code: code.into(),
            message: message.into(),
        }),
    }
}

async fn transcribe(
    request_id: String,
    state: &AppState,
    audio_path: std::path::PathBuf,
) -> Response {
    if state.status.read().await.job_state != JobState::Idle {
        return state_error(request_id, state, "busy", "VoxLine is busy").await;
    }
    update_model_state(state, ModelState::Loading).await;
    update_state(state, None, JobState::Transcribing).await;
    match state.asr.transcribe(audio_path).await {
        Ok((transcript, benchmark)) => {
            update_model_state(state, ModelState::Ready).await;
            let status = update_state(state, None, JobState::Idle).await;
            data_response(request_id, status, None, Some(transcript), Some(benchmark))
        }
        Err(error) => asr_error_response(request_id, state, error).await,
    }
}

async fn asr_load(request_id: String, state: &AppState) -> Response {
    update_model_state(state, ModelState::Loading).await;
    match state.asr.load().await {
        Ok(model_load_ms) => {
            let status = update_model_state(state, ModelState::Ready).await;
            data_response(
                request_id,
                status,
                None,
                None,
                Some(AsrBenchmark {
                    model_load_ms,
                    transcribe_ms: 0,
                    audio_duration_ms: 0,
                }),
            )
        }
        Err(error) => asr_error_response(request_id, state, error).await,
    }
}

async fn asr_unload(request_id: String, state: &AppState) -> Response {
    match state.asr.unload().await {
        Ok(()) => {
            let status = update_model_state(state, ModelState::Unloaded).await;
            ok_response(request_id, status)
        }
        Err(error) => asr_error_response(request_id, state, error).await,
    }
}

async fn asr_restart(request_id: String, state: &AppState) -> Response {
    if let Err(error) = state.asr.unload().await {
        return asr_error_response(request_id, state, error).await;
    }
    asr_load(request_id, state).await
}

async fn update_model_state(state: &AppState, model_state: ModelState) -> DaemonStatus {
    let mut status = state.status.write().await;
    status.final_model_state.clone_from(&model_state);
    let snapshot = status.clone();
    let _ = state.events.send(Event::State {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        job_id: snapshot.active_job_id.clone(),
        job_state: snapshot.job_state.clone(),
        final_model_state: model_state,
    });
    snapshot
}

async fn asr_error_response(
    request_id: String,
    state: &AppState,
    error: asr::AsrError,
) -> Response {
    let message = error.to_string();
    update_model_state(
        state,
        ModelState::Failed {
            code: "asr_error".into(),
            message: message.clone(),
        },
    )
    .await;
    let status = update_state(state, None, JobState::Idle).await;
    emit_error(state, None, "asr_error", &message);
    error_response(request_id, "asr_error", &message, Some(status))
}

async fn toggle(request_id: String, state: &AppState) -> Response {
    match state.status.read().await.job_state {
        JobState::Idle => start(request_id, state).await,
        JobState::Recording => stop(request_id, state).await,
        _ => state_error(request_id, state, "busy", "VoxLine is busy").await,
    }
}

async fn start(request_id: String, state: &AppState) -> Response {
    if state.status.read().await.job_state != JobState::Idle {
        return state_error(request_id, state, "busy", "VoxLine is busy").await;
    }
    let job_id = JobId::new();
    match state.audio.start(job_id.clone()).await {
        Ok(()) => {
            let status = update_state(state, Some(job_id), JobState::Recording).await;
            ok_response(request_id, status)
        }
        Err(error) => audio_error_response(request_id, state, error).await,
    }
}

async fn stop(request_id: String, state: &AppState) -> Response {
    let job_id = {
        let status = state.status.read().await;
        if status.job_state != JobState::Recording {
            drop(status);
            return state_error(
                request_id,
                state,
                "no_active_recording",
                "there is no active recording",
            )
            .await;
        }
        status
            .active_job_id
            .clone()
            .expect("recording has a job id")
    };
    update_state(state, Some(job_id.clone()), JobState::Stopping).await;
    match state.audio.stop(job_id).await {
        Ok(recording) => {
            let status = update_state(state, None, JobState::Idle).await;
            success_response(request_id, status, Some(recording))
        }
        Err(error) => audio_error_response(request_id, state, error).await,
    }
}

async fn cancel(request_id: String, state: &AppState) -> Response {
    let job_id = {
        let status = state.status.read().await;
        if status.job_state != JobState::Recording {
            drop(status);
            return state_error(
                request_id,
                state,
                "no_active_recording",
                "there is no active recording",
            )
            .await;
        }
        status
            .active_job_id
            .clone()
            .expect("recording has a job id")
    };
    match state.audio.cancel(job_id.clone()).await {
        Ok(()) => {
            update_state(state, Some(job_id), JobState::Cancelled).await;
            let status = update_state(state, None, JobState::Idle).await;
            ok_response(request_id, status)
        }
        Err(error) => audio_error_response(request_id, state, error).await,
    }
}

async fn update_state(
    state: &AppState,
    job_id: Option<JobId>,
    job_state: JobState,
) -> DaemonStatus {
    let mut status = state.status.write().await;
    status.active_job_id.clone_from(&job_id);
    status.job_state.clone_from(&job_state);
    let snapshot = status.clone();
    let _ = state.events.send(Event::State {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        job_id,
        job_state,
        final_model_state: snapshot.final_model_state.clone(),
    });
    snapshot
}

async fn state_error(request_id: String, state: &AppState, code: &str, message: &str) -> Response {
    let status = state.status.read().await.clone();
    emit_error(state, status.active_job_id.clone(), code, message);
    error_response(request_id, code, message, Some(status))
}

async fn audio_error_response(
    request_id: String,
    state: &AppState,
    error: audio::AudioError,
) -> Response {
    emit_error(state, None, "audio_error", &error.to_string());
    let status = update_state(state, None, JobState::Idle).await;
    error_response(request_id, "audio_error", &error.to_string(), Some(status))
}

fn emit_error(state: &AppState, job_id: Option<JobId>, code: &str, message: &str) {
    let _ = state.events.send(Event::Error {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        job_id,
        error: ProtocolError {
            code: code.into(),
            message: message.into(),
        },
    });
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
