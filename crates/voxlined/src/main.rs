mod asr;
mod audio;
mod cleanup;
mod injection;
mod openrouter;
mod preview;
mod preview_asr;
mod template_extract;

use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use clap::Parser;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    signal,
    sync::{Mutex, RwLock, broadcast},
};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use voxline_core::{
    cleanup::{CleanupOverride, should_run_cleanup_with_voice_style},
    config::{
        AudioGatesConfig, AutoPasteMode, CleanupConfig, Config, InjectionConfig,
        NotificationsConfig, PathsConfig, PrivacyConfig, SecretsConfig, VoiceCommandsConfig,
    },
    protocol::{
        AsrBenchCandidate, AsrBenchmark, AudioRecording, Command, DaemonStatus, DictationResult,
        Event, EventKind, JobId, JobState, ModelBenchResult, ModelState, PROTOCOL_VERSION,
        ProtocolError, Request, Response, SessionEnvironment, Transcript,
    },
    runtime::{ensure_runtime_dir_for, secure_socket_permissions, socket_path_for},
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
    preview: preview::PreviewCoordinator,
    preview_asr: Option<preview_asr::PreviewAsrManager>,
    audio: audio::AudioRecorder,
    asr: asr::AsrManager,
    audio_gates: AudioGatesConfig,
    injection: InjectionConfig,
    notifications: NotificationsConfig,
    privacy: PrivacyConfig,
    target_at_start: Mutex<Option<voxline_platform::TargetContext>>,
    cleanup_override: Mutex<Option<CleanupOverride>>,
    style_override: Mutex<Option<String>>,
    active_app_profile: Mutex<Option<voxline_core::apps::AppProfile>>,
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

    ensure_runtime_dir_for(&config.paths)?;
    let socket = socket_path_for(&config.paths)?;
    remove_stale_socket(&socket)?;
    let listener = UnixListener::bind(&socket)
        .with_context(|| format!("failed to bind {}", socket.display()))?;
    secure_socket_permissions(&socket).context("failed to secure daemon socket permissions")?;
    let (events, _) = broadcast::channel(32);
    let audio_gates = config.audio.gates.clone();
    let paste_available = voxline_platform::paste_backend().is_some();
    let auto_paste_effective = match (&config.injection.auto_paste, paste_available) {
        (AutoPasteMode::Off, _) | (_, false) => "clipboard_only",
        (AutoPasteMode::Safe, true) => "safe",
        (AutoPasteMode::Always, true) => "always",
    };
    let preview_enabled = config.preview.enabled;
    let preview_asr = preview_enabled
        .then(|| preview_asr::PreviewAsrManager::spawn(&config.preview, &config.asr));
    let state = Arc::new(AppState {
        status: RwLock::new(DaemonStatus {
            cleanup_enabled: config.cleanup.enabled,
            asr_gpu_build: cfg!(feature = "asr-whisper-rs-cuda"),
            auto_paste_effective: auto_paste_effective.into(),
            preview_model_state: preview_enabled.then_some(ModelState::Unloaded),
            ..DaemonStatus::default()
        }),
        events,
        preview: preview::PreviewCoordinator::new(config.preview.clone()),
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
                    session_environment: None,
                    cleaned_text: None,
                    cleanup_ms: None,
                    dictation: None,
                    model_bench_results: None,
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
        if let Command::Subscribe { events } = request.command {
            let response = ok_response(request.request_id, state.status.read().await.clone());
            write_json_line(&mut writer, &response).await?;
            stream_subscribe(&mut writer, &state, events).await?;
            return Ok(());
        }
        let response = dispatch(request, Arc::clone(&state)).await;
        write_json_line(&mut writer, &response).await?;
    }
    Ok(())
}

async fn dispatch(request: Request, state: Arc<AppState>) -> Response {
    match request.command {
        Command::Status | Command::AsrStatus => {
            ok_response(request.request_id, state.status.read().await.clone())
        }
        Command::Toggle {
            cleanup,
            style,
            snippet,
        } => {
            toggle(
                request.request_id,
                Arc::clone(&state),
                cleanup,
                style,
                snippet,
            )
            .await
        }
        Command::InsertSnippet { name } => insert_snippet(request.request_id, &state, name).await,
        Command::TemplatePreview { name, text } => {
            template_preview(request.request_id, &state, name, text).await
        }
        Command::Start => start(request.request_id, state, None, None).await,
        Command::Stop => stop(request.request_id, &state).await,
        Command::Cancel => cancel(request.request_id, &state).await,
        Command::Transcribe { audio_path } => {
            transcribe(request.request_id, &state, audio_path).await
        }
        Command::BenchDictation {
            audio_path,
            cleanup,
            attempt_paste,
        } => {
            bench_dictation(
                request.request_id,
                &state,
                audio_path,
                cleanup,
                attempt_paste,
            )
            .await
        }
        Command::SetupRecord {
            seconds,
            output_path,
        } => setup_record(request.request_id, &state, seconds, output_path).await,
        Command::BenchModelCompare {
            audio_path,
            candidates,
            include_cold_load,
        } => {
            bench_model_compare(
                request.request_id,
                &state,
                audio_path,
                candidates,
                include_cold_load,
            )
            .await
        }
        Command::AsrLoad => asr_load(request.request_id, &state).await,
        Command::AsrUnload => asr_unload(request.request_id, &state).await,
        Command::AsrRestart => asr_restart(request.request_id, &state).await,
        Command::TestClipboard => test_clipboard(request.request_id, &state).await,
        Command::TestPaste => test_paste(request.request_id, &state).await,
        Command::TestOpenrouter => test_openrouter(request.request_id, &state).await,
        Command::CleanupPreview { text, style } => {
            cleanup_preview(request.request_id, &state, text, style).await
        }
        Command::DaemonEnvironment => {
            daemon_environment_response(request.request_id, state.status.read().await.clone())
        }
        Command::Subscribe { .. } => unreachable!("subscribe handled before dispatch"),
    }
}

fn daemon_environment_response(request_id: String, status: DaemonStatus) -> Response {
    Response {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: true,
        status: Some(status),
        recording: None,
        transcript: None,
        benchmark: None,
        error: None,
        session_environment: Some(current_session_environment()),
        cleaned_text: None,
        cleanup_ms: None,
        dictation: None,
        model_bench_results: None,
    }
}

fn current_session_environment() -> SessionEnvironment {
    let report = voxline_platform::environment_report();
    SessionEnvironment {
        session_type: report.session_type,
        desktop: report.desktop,
        wayland_display_present: report.wayland_display_present,
        display_present: report.display_present,
        dbus_session_bus_present: report.dbus_session_bus_present,
        xdg_runtime_dir_present: report.xdg_runtime_dir_present,
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
    data_response_with(
        request_id, status, recording, transcript, benchmark, None, None,
    )
}

fn data_response_with(
    request_id: String,
    status: DaemonStatus,
    recording: Option<AudioRecording>,
    transcript: Option<Transcript>,
    benchmark: Option<AsrBenchmark>,
    cleanup_ms: Option<u64>,
    dictation: Option<DictationResult>,
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
        session_environment: None,
        cleaned_text: None,
        cleanup_ms,
        dictation,
        model_bench_results: None,
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
        session_environment: None,
        cleaned_text: None,
        cleanup_ms: None,
        dictation: None,
        model_bench_results: None,
    }
}

fn reload_job_config() -> (
    CleanupConfig,
    PathsConfig,
    SecretsConfig,
    bool,
    VoiceCommandsConfig,
) {
    Config::load_or_default().map_or_else(
        |_| {
            (
                CleanupConfig::default(),
                PathsConfig::default(),
                SecretsConfig::default(),
                false,
                VoiceCommandsConfig::default(),
            )
        },
        |config| {
            (
                config.cleanup.clone(),
                config.paths.clone(),
                config.secrets.clone(),
                config.cleanup.enabled,
                config.voice_commands.clone(),
            )
        },
    )
}

async fn bench_dictation(
    request_id: String,
    state: &AppState,
    audio_path: std::path::PathBuf,
    cleanup: Option<voxline_core::cleanup::CleanupOverride>,
    attempt_paste: bool,
) -> Response {
    if state.status.read().await.job_state != JobState::Idle {
        return state_error(request_id, state, "busy", "VoxLine is busy").await;
    }
    let config = Config::load_or_default().unwrap_or_default();
    let recording = match audio::recording_from_existing_wav(
        &audio_path,
        &config.audio.gates,
        config.audio.target_sample_rate,
    ) {
        Ok(recording) => recording,
        Err(error) => {
            return error_response(
                request_id,
                "bench_audio_invalid",
                &error.to_string(),
                Some(state.status.read().await.clone()),
            );
        }
    };
    if let Some(cleanup) = cleanup {
        *state.cleanup_override.lock().await = Some(cleanup);
    }
    let target_at_stop = if attempt_paste {
        voxline_platform::capture_active_target()
    } else {
        None
    };
    finish_dictation(
        request_id,
        state,
        recording,
        target_at_stop,
        Instant::now(),
        attempt_paste,
        true,
    )
    .await
}

async fn setup_record(
    request_id: String,
    state: &AppState,
    seconds: u64,
    output_path: std::path::PathBuf,
) -> Response {
    if state.status.read().await.job_state != JobState::Idle {
        return state_error(request_id, state, "busy", "VoxLine is busy").await;
    }
    let job_id = JobId::new();
    if let Err(error) = state.audio.start(job_id.clone(), None).await {
        return audio_error_response(request_id, state, error).await;
    }
    let status = update_state(state, Some(job_id.clone()), JobState::Recording).await;
    let _ = status;
    tokio::time::sleep(Duration::from_secs(seconds.max(1))).await;
    state.preview.stop().await;
    let recording = match state.audio.stop(job_id).await {
        Ok(recording) => recording,
        Err(error) => {
            let _ = update_state(state, None, JobState::Idle).await;
            return audio_error_response(request_id, state, error).await;
        }
    };
    let status = update_state(state, None, JobState::Idle).await;
    if !recording.speech_detected {
        return error_response(
            request_id,
            "no_speech",
            "recording was too short or quiet to transcribe",
            Some(status),
        );
    }
    if let Some(parent) = output_path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return error_response(
            request_id,
            "setup_record_failed",
            &error.to_string(),
            Some(status),
        );
    }
    if let Err(error) = fs::copy(&recording.wav_path, &output_path) {
        return error_response(
            request_id,
            "setup_record_failed",
            &error.to_string(),
            Some(status),
        );
    }
    let _ = fs::remove_file(&recording.wav_path);
    let mut saved = recording;
    saved.wav_path = output_path;
    success_response(request_id, status, Some(saved))
}

async fn bench_model_compare(
    request_id: String,
    state: &AppState,
    audio_path: std::path::PathBuf,
    candidates: Vec<AsrBenchCandidate>,
    include_cold_load: bool,
) -> Response {
    if state.status.read().await.job_state != JobState::Idle {
        return state_error(request_id, state, "busy", "VoxLine is busy").await;
    }
    if candidates.is_empty() {
        return error_response(
            request_id,
            "bench_no_candidates",
            "no models were provided for comparison",
            Some(state.status.read().await.clone()),
        );
    }
    let base_config = Config::load_or_default().unwrap_or_default().asr;
    let mut results = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let mut asr_config = base_config.clone();
        asr_config.model_path = candidate.model_path.display().to_string();
        asr_config.gpu = candidate.gpu;
        asr_config.lifecycle.mode = "on_demand".into();
        asr_config.lifecycle.idle_unload_seconds = 0;

        let cold_load_ms = if include_cold_load {
            let _ = state.asr.unload().await;
            match state.asr.reload(asr_config.clone()).await {
                Ok(ms) => ms,
                Err(error) => {
                    results.push(ModelBenchResult {
                        model_id: candidate.model_id.clone(),
                        cold_load_ms: 0,
                        warm_transcribe_ms: 0,
                        audio_duration_ms: 0,
                        transcript_text: String::new(),
                        error: Some(error.to_string()),
                    });
                    continue;
                }
            }
        } else {
            match state.asr.reload(asr_config.clone()).await {
                Ok(ms) => ms,
                Err(error) => {
                    results.push(ModelBenchResult {
                        model_id: candidate.model_id.clone(),
                        cold_load_ms: 0,
                        warm_transcribe_ms: 0,
                        audio_duration_ms: 0,
                        transcript_text: String::new(),
                        error: Some(error.to_string()),
                    });
                    continue;
                }
            }
        };

        match state.asr.transcribe(audio_path.clone()).await {
            Ok((transcript, benchmark)) => {
                results.push(ModelBenchResult {
                    model_id: candidate.model_id,
                    cold_load_ms,
                    warm_transcribe_ms: benchmark.transcribe_ms,
                    audio_duration_ms: benchmark.audio_duration_ms,
                    transcript_text: transcript.text,
                    error: None,
                });
            }
            Err(error) => {
                results.push(ModelBenchResult {
                    model_id: candidate.model_id,
                    cold_load_ms,
                    warm_transcribe_ms: 0,
                    audio_duration_ms: 0,
                    transcript_text: String::new(),
                    error: Some(error.to_string()),
                });
            }
        }
        let _ = state.asr.unload().await;
    }
    let _ = state.asr.reload(base_config).await;
    let status = state.status.read().await.clone();
    Response {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: true,
        status: Some(status),
        recording: None,
        transcript: None,
        benchmark: None,
        error: None,
        session_environment: None,
        cleaned_text: None,
        cleanup_ms: None,
        dictation: None,
        model_bench_results: Some(results),
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

async fn toggle(
    request_id: String,
    state: Arc<AppState>,
    cleanup: Option<CleanupOverride>,
    style: Option<String>,
    snippet: Option<String>,
) -> Response {
    let job_state = state.status.read().await.job_state.clone();
    match job_state {
        JobState::Idle => {
            if let Some(name) = snippet
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                return insert_snippet(request_id, &state, name.into()).await;
            }
            start(request_id, state, cleanup, style).await
        }
        JobState::Recording => stop(request_id, &state).await,
        _ => state_error(request_id, &state, "busy", "VoxLine is busy").await,
    }
}

async fn start(
    request_id: String,
    state: Arc<AppState>,
    cleanup: Option<CleanupOverride>,
    style: Option<String>,
) -> Response {
    if state.status.read().await.job_state != JobState::Idle {
        return state_error(request_id, &state, "busy", "VoxLine is busy").await;
    }
    let job_id = JobId::new();
    *state.cleanup_override.lock().await = cleanup;
    let target_at_start = voxline_platform::capture_active_target();
    let active_app_profile = Config::load_or_default().ok().and_then(|config| {
        target_at_start.as_ref().and_then(|target| {
            voxline_core::apps::match_app_profile(
                &config.paths,
                target.app_id.as_deref(),
                target.title.as_deref(),
            )
        })
    });
    *state.target_at_start.lock().await = target_at_start;
    *state.active_app_profile.lock().await = active_app_profile;
    *state.style_override.lock().await = style;
    let preview_ring_buffer_seconds = state.preview.is_enabled().then(|| {
        Config::load_or_default()
            .map(|config| config.preview.ring_buffer_seconds)
            .unwrap_or(30)
    });
    match state
        .audio
        .start(job_id.clone(), preview_ring_buffer_seconds)
        .await
    {
        Ok(()) => {
            if let (Some(tap), Some(preview_asr)) =
                (state.audio.current_tap(), state.preview_asr.clone())
            {
                let events = state.events.clone();
                let state_for_preview = Arc::clone(&state);
                state
                    .preview
                    .start(
                        job_id.clone(),
                        tap,
                        preview_asr,
                        events,
                        Arc::new(move |model_state| {
                            let state = Arc::clone(&state_for_preview);
                            tokio::spawn(async move {
                                update_preview_model_state(&state, model_state).await;
                            });
                        }),
                    )
                    .await;
            }
            let status = update_state(&state, Some(job_id), JobState::Recording).await;
            ok_response(request_id, status)
        }
        Err(error) => audio_error_response(request_id, &state, error).await,
    }
}

async fn stop(request_id: String, state: &AppState) -> Response {
    let started = Instant::now();
    let target_at_stop = voxline_platform::capture_active_target();
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
    state.preview.stop().await;
    if state.preview.is_enabled() {
        update_preview_model_state(state, ModelState::Unloaded).await;
    }
    update_state(state, Some(job_id.clone()), JobState::Stopping).await;
    match state.audio.stop(job_id).await {
        Ok(recording) => {
            finish_dictation(
                request_id,
                state,
                recording,
                target_at_stop,
                started,
                true,
                false,
            )
            .await
        }
        Err(error) => audio_error_response(request_id, state, error).await,
    }
}

#[allow(clippy::too_many_lines)]
async fn finish_dictation(
    request_id: String,
    state: &AppState,
    recording: AudioRecording,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    attempt_paste: bool,
    retain_audio_file: bool,
) -> Response {
    let _audio_cleanup = TemporaryAudio::new(
        recording.wav_path.clone(),
        state.privacy.store_audio || retain_audio_file,
    );
    if !recording.speech_detected {
        if state.notifications.enabled && state.audio_gates.notify_on_no_speech {
            voxline_platform::notify("VoxLine", "No speech detected");
        }
        let status = update_state(state, None, JobState::Idle).await;
        return error_response(
            request_id,
            "no_speech",
            "recording was too short or quiet to transcribe",
            Some(status),
        );
    }

    update_model_state(state, ModelState::Loading).await;
    update_state(
        state,
        Some(recording.job_id.clone()),
        JobState::Transcribing,
    )
    .await;
    let (transcript, benchmark) = match state.asr.transcribe(recording.wav_path.clone()).await {
        Ok(result) => result,
        Err(error) => return asr_error_response(request_id, state, error).await,
    };
    update_model_state(state, ModelState::Ready).await;
    if transcript.text.trim().is_empty() {
        if state.notifications.enabled {
            voxline_platform::notify("VoxLine", "No speech recognized");
        }
        let status = update_state(state, None, JobState::Idle).await;
        return error_response(
            request_id,
            "empty_transcript",
            "transcription produced no usable text",
            Some(status),
        );
    }

    let cleanup_override = state.cleanup_override.lock().await.take();
    let style_override = state.style_override.lock().await.take();
    let active_app_profile = state.active_app_profile.lock().await.take();
    let (cleanup_config, paths_config, secrets_config, cleanup_enabled, voice_commands_config) =
        reload_job_config();
    let prefer_clipboard_only = active_app_profile
        .as_ref()
        .and_then(|profile| profile.injection.prefer_clipboard_only)
        .unwrap_or(false);
    let voice_command_outcome = apply_voice_commands(
        &voice_commands_config,
        &paths_config,
        &transcript.text,
        state.privacy.log_transcripts,
    );
    let voice_style_override = voice_command_outcome.voice_style;
    let raw_text = voice_command_outcome.raw_text;
    if let Some(name) = voice_command_outcome.insert_snippet_only {
        return deliver_snippet_from_job(
            request_id,
            state,
            SnippetJobContext {
                recording,
                benchmark,
                target_at_stop,
                started,
                paths_config,
                name,
                prefer_clipboard_only,
            },
        )
        .await;
    }
    if let Some(name) = voice_command_outcome.template_snippet {
        return deliver_template_from_job(
            request_id,
            state,
            TemplateJobContext {
                recording,
                benchmark,
                target_at_stop,
                started,
                cleanup_config,
                paths_config,
                secrets_config,
                name,
                input: raw_text.clone(),
                prefer_clipboard_only,
            },
        )
        .await;
    }
    if raw_text.trim().is_empty() {
        let status = update_state(state, None, JobState::Idle).await;
        return error_response(
            request_id,
            "empty_transcript",
            "transcription produced no usable text after command stripping",
            Some(status),
        );
    }
    let routing = voxline_core::routing::resolve_cleanup_routing(
        style_override.as_deref(),
        voice_style_override.as_deref(),
        cleanup_override,
        cleanup_enabled,
        &cleanup_config.default_style,
        active_app_profile.as_ref(),
    );
    let cleanup_outcome = if should_run_cleanup_with_voice_style(
        routing.cleanup_enabled,
        cleanup_override,
        &raw_text,
        cleanup_config.skip_if_word_count_below,
        voice_style_override.is_some(),
    ) {
        update_state(state, Some(recording.job_id.clone()), JobState::Cleaning).await;
        match cleanup::run_cleanup(
            &cleanup_config,
            &paths_config,
            &secrets_config,
            &routing.style_name,
            routing.app_prompt.as_deref(),
            &raw_text,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                warn!(%error, "cleanup failed; falling back to raw transcript");
                if cleanup_config.fallback_to_raw_on_error {
                    cleanup::failed_fallback_outcome(raw_text)
                } else {
                    let status = update_state(state, None, JobState::Idle).await;
                    return error_response(
                        request_id,
                        "cleanup_error",
                        &error.to_string(),
                        Some(status),
                    );
                }
            }
        }
    } else {
        cleanup::passthrough_outcome(raw_text)
    };
    let final_text = cleanup_outcome.text.clone();
    let mut final_transcript = transcript.clone();
    final_transcript.text = final_text.clone();
    {
        let mut status = state.status.write().await;
        status.cleanup_enabled = cleanup_enabled;
    }

    let delivery = match deliver_text_to_target(
        state,
        &recording.job_id,
        &final_text,
        target_at_stop,
        started,
        routing.prefer_clipboard_only || !attempt_paste,
    )
    .await
    {
        Ok(delivery) => delivery,
        Err(message) => {
            let status = update_state(state, None, JobState::Idle).await;
            emit_error(state, Some(recording.job_id), "clipboard_error", &message);
            return error_response(request_id, "clipboard_error", &message, Some(status));
        }
    };
    let copied_to_clipboard = delivery.copied_to_clipboard;
    let paste_outcome = delivery.paste_outcome;
    let clipboard_restored = delivery.clipboard_restored;

    let result = DictationResult {
        job_id: recording.job_id.clone(),
        transcript: final_transcript.clone(),
        benchmark: benchmark.clone(),
        total_ms: elapsed_ms(started),
        copied_to_clipboard,
        pasted: paste_outcome.paste_succeeded,
        paste_attempted: paste_outcome.paste_attempted,
        paste_succeeded: paste_outcome.paste_succeeded,
        clipboard_restored,
        cleanup_used: cleanup_outcome.used,
        cleanup_failed: cleanup_outcome.failed,
        snippet_used: None,
        insertion_reason: paste_outcome.insertion_reason.clone(),
    };
    let _ = state.events.send(Event::Result {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        result: result.clone(),
    });
    if state.notifications.enabled {
        voxline_platform::notify(
            "VoxLine",
            if paste_outcome.paste_succeeded {
                "Paste command sent"
            } else if copied_to_clipboard {
                "Transcript copied to clipboard"
            } else {
                "Transcription complete"
            },
        );
    }
    update_state(state, Some(recording.job_id.clone()), JobState::Done).await;
    let status = update_state(state, None, JobState::Idle).await;
    data_response_with(
        request_id,
        status,
        Some(recording),
        Some(final_transcript),
        Some(benchmark),
        if cleanup_outcome.used {
            Some(cleanup_outcome.cleanup_ms)
        } else {
            None
        },
        Some(result),
    )
}

async fn test_openrouter(request_id: String, state: &AppState) -> Response {
    let (cleanup_config, paths_config, secrets_config, _, _) = reload_job_config();
    if cleanup_config.provider != "openrouter" {
        return state_error(
            request_id,
            state,
            "openrouter_test_unavailable",
            "cleanup provider is not set to openrouter",
        )
        .await;
    }
    let routing = voxline_core::routing::resolve_cleanup_routing(
        None,
        None,
        None,
        cleanup_config.enabled,
        &cleanup_config.default_style,
        None,
    );
    match cleanup::run_cleanup(
        &cleanup_config,
        &paths_config,
        &secrets_config,
        &routing.style_name,
        routing.app_prompt.as_deref(),
        "VoxLine OpenRouter test",
    )
    .await
    {
        Ok(outcome) => {
            let status = state.status.read().await.clone();
            Response {
                protocol_version: PROTOCOL_VERSION,
                request_id,
                ok: true,
                status: Some(status),
                recording: None,
                transcript: None,
                benchmark: None,
                error: None,
                session_environment: None,
                cleaned_text: Some(outcome.text),
                cleanup_ms: Some(outcome.cleanup_ms),
                dictation: None,
                model_bench_results: None,
            }
        }
        Err(error) => {
            state_error(
                request_id,
                state,
                "openrouter_test_failed",
                &error.to_string(),
            )
            .await
        }
    }
}

async fn cleanup_preview(
    request_id: String,
    state: &AppState,
    text: String,
    style: Option<String>,
) -> Response {
    let (cleanup_config, paths_config, secrets_config, cleanup_enabled, _) = reload_job_config();
    if !cleanup_enabled && cleanup_config.provider == "none" {
        return state_error(
            request_id,
            state,
            "cleanup_disabled",
            "cleanup is disabled; run voxline cleanup enable openrouter",
        )
        .await;
    }
    let routing = voxline_core::routing::resolve_cleanup_routing(
        style.as_deref(),
        None,
        None,
        cleanup_enabled,
        &cleanup_config.default_style,
        None,
    );
    match cleanup::run_cleanup(
        &cleanup_config,
        &paths_config,
        &secrets_config,
        &routing.style_name,
        routing.app_prompt.as_deref(),
        &text,
    )
    .await
    {
        Ok(outcome) => {
            let status = state.status.read().await.clone();
            Response {
                protocol_version: PROTOCOL_VERSION,
                request_id,
                ok: true,
                status: Some(status),
                recording: None,
                transcript: None,
                benchmark: None,
                error: None,
                session_environment: None,
                cleaned_text: Some(outcome.text),
                cleanup_ms: Some(outcome.cleanup_ms),
                dictation: None,
                model_bench_results: None,
            }
        }
        Err(error) => {
            state_error(
                request_id,
                state,
                "cleanup_preview_failed",
                &error.to_string(),
            )
            .await
        }
    }
}

struct SnippetJobContext {
    recording: AudioRecording,
    benchmark: AsrBenchmark,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    paths_config: PathsConfig,
    name: String,
    prefer_clipboard_only: bool,
}

struct VoiceCommandOutcome {
    raw_text: String,
    voice_style: Option<String>,
    insert_snippet_only: Option<String>,
    template_snippet: Option<String>,
}

fn apply_voice_commands(
    voice_commands_config: &VoiceCommandsConfig,
    paths_config: &PathsConfig,
    transcript: &str,
    log_transcripts: bool,
) -> VoiceCommandOutcome {
    let mut outcome = VoiceCommandOutcome {
        raw_text: transcript.to_owned(),
        voice_style: None,
        insert_snippet_only: None,
        template_snippet: None,
    };
    if !voice_commands_config.enabled {
        return outcome;
    }
    let Ok(registry) = voxline_core::commands::build_command_registry(paths_config) else {
        tracing::debug!("voice command registry unavailable");
        return outcome;
    };
    let Some(parsed) = voxline_core::commands::parse_voice_command(
        voice_commands_config,
        &registry,
        transcript.trim(),
    ) else {
        if log_transcripts {
            tracing::debug!(transcript, "no voice command matched transcript");
        }
        return outcome;
    };
    outcome.raw_text.clone_from(&parsed.remainder);
    match parsed.target {
        voxline_core::commands::CommandTarget::Style { name } => {
            outcome.voice_style = Some(name);
        }
        voxline_core::commands::CommandTarget::Snippet { name } => {
            match voxline_core::snippets::snippet_kind(paths_config, &name) {
                Ok(voxline_core::snippets::SnippetKind::Insert)
                    if outcome.raw_text.trim().is_empty() =>
                {
                    outcome.insert_snippet_only = Some(name);
                }
                Ok(voxline_core::snippets::SnippetKind::Template)
                    if !outcome.raw_text.trim().is_empty() =>
                {
                    outcome.template_snippet = Some(name);
                }
                _ => {}
            }
        }
    }
    outcome
}

struct TemplateJobContext {
    recording: AudioRecording,
    benchmark: AsrBenchmark,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    cleanup_config: CleanupConfig,
    paths_config: PathsConfig,
    secrets_config: SecretsConfig,
    name: String,
    input: String,
    prefer_clipboard_only: bool,
}

#[allow(clippy::too_many_lines)]
async fn deliver_template_from_job(
    request_id: String,
    state: &AppState,
    job: TemplateJobContext,
) -> Response {
    let TemplateJobContext {
        recording,
        benchmark,
        target_at_stop,
        started,
        cleanup_config,
        paths_config,
        secrets_config,
        name,
        input,
        prefer_clipboard_only,
    } = job;
    let metadata =
        match voxline_core::snippet_templates::load_template_metadata(&paths_config, &name) {
            Ok(metadata) => metadata,
            Err(error) => {
                let status = update_state(state, None, JobState::Idle).await;
                return error_response(
                    request_id,
                    "template_error",
                    &error.to_string(),
                    Some(status),
                );
            }
        };
    update_state(state, Some(recording.job_id.clone()), JobState::Cleaning).await;
    let render_outcome = match template_extract::run_template_snippet(
        &cleanup_config,
        &paths_config,
        &secrets_config,
        &metadata,
        &input,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            warn!(%error, "template extraction failed");
            if template_extract::should_use_raw_fallback(&metadata) {
                template_extract::raw_fallback_outcome(input)
            } else {
                let status = update_state(state, None, JobState::Idle).await;
                return error_response(
                    request_id,
                    "template_error",
                    &error.to_string(),
                    Some(status),
                );
            }
        }
    };
    let delivery = match deliver_text_to_target(
        state,
        &recording.job_id,
        &render_outcome.text,
        target_at_stop,
        started,
        prefer_clipboard_only,
    )
    .await
    {
        Ok(delivery) => delivery,
        Err(message) => {
            let status = update_state(state, None, JobState::Idle).await;
            emit_error(
                state,
                Some(recording.job_id.clone()),
                "clipboard_error",
                &message,
            );
            return error_response(request_id, "clipboard_error", &message, Some(status));
        }
    };
    let final_transcript = Transcript {
        text: render_outcome.text.clone(),
        language: None,
        duration_ms: Some(recording.duration_ms),
        segments: Vec::new(),
    };
    let result = DictationResult {
        job_id: recording.job_id.clone(),
        transcript: final_transcript.clone(),
        benchmark: benchmark.clone(),
        total_ms: elapsed_ms(started),
        copied_to_clipboard: delivery.copied_to_clipboard,
        pasted: delivery.paste_outcome.paste_succeeded,
        paste_attempted: delivery.paste_outcome.paste_attempted,
        paste_succeeded: delivery.paste_outcome.paste_succeeded,
        clipboard_restored: delivery.clipboard_restored,
        cleanup_used: render_outcome.used_extraction,
        cleanup_failed: render_outcome.failed,
        snippet_used: Some(name),
        insertion_reason: delivery.paste_outcome.insertion_reason.clone(),
    };
    let _ = state.events.send(Event::Result {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        result,
    });
    if state.notifications.enabled {
        voxline_platform::notify(
            "VoxLine",
            if delivery.paste_outcome.paste_succeeded {
                "Template paste command sent"
            } else if delivery.copied_to_clipboard {
                "Template copied to clipboard"
            } else {
                "Template ready"
            },
        );
    }
    update_state(state, Some(recording.job_id.clone()), JobState::Done).await;
    let status = update_state(state, None, JobState::Idle).await;
    data_response(
        request_id,
        status,
        Some(recording),
        Some(final_transcript),
        Some(benchmark),
    )
}

async fn template_preview(
    request_id: String,
    state: &AppState,
    name: String,
    text: String,
) -> Response {
    let (cleanup_config, paths_config, secrets_config, cleanup_enabled, _) = reload_job_config();
    if !cleanup_enabled || cleanup_config.provider != "openrouter" {
        return state_error(
            request_id,
            state,
            "template_preview_unavailable",
            "template extraction requires enabled openrouter cleanup",
        )
        .await;
    }
    let metadata =
        match voxline_core::snippet_templates::load_template_metadata(&paths_config, &name) {
            Ok(metadata) => metadata,
            Err(error) => {
                return state_error(request_id, state, "template_error", &error.to_string()).await;
            }
        };
    match template_extract::run_template_snippet(
        &cleanup_config,
        &paths_config,
        &secrets_config,
        &metadata,
        &text,
    )
    .await
    {
        Ok(outcome) => {
            let status = state.status.read().await.clone();
            Response {
                protocol_version: PROTOCOL_VERSION,
                request_id,
                ok: true,
                status: Some(status),
                recording: None,
                transcript: None,
                benchmark: None,
                error: None,
                session_environment: None,
                cleaned_text: Some(outcome.text),
                cleanup_ms: Some(outcome.extract_ms),
                dictation: None,
                model_bench_results: None,
            }
        }
        Err(error) => {
            state_error(
                request_id,
                state,
                "template_preview_failed",
                &error.to_string(),
            )
            .await
        }
    }
}

async fn deliver_snippet_from_job(
    request_id: String,
    state: &AppState,
    job: SnippetJobContext,
) -> Response {
    let SnippetJobContext {
        recording,
        benchmark,
        target_at_stop,
        started,
        paths_config,
        name,
        prefer_clipboard_only,
    } = job;
    let snippet_text = match voxline_core::snippets::load_snippet_content(&paths_config, &name) {
        Ok(snippet_text) => snippet_text,
        Err(error) => {
            let status = update_state(state, None, JobState::Idle).await;
            return error_response(
                request_id,
                "snippet_error",
                &error.to_string(),
                Some(status),
            );
        }
    };
    let delivery = match deliver_text_to_target(
        state,
        &recording.job_id,
        &snippet_text,
        target_at_stop,
        started,
        prefer_clipboard_only,
    )
    .await
    {
        Ok(delivery) => delivery,
        Err(message) => {
            let status = update_state(state, None, JobState::Idle).await;
            emit_error(
                state,
                Some(recording.job_id.clone()),
                "clipboard_error",
                &message,
            );
            return error_response(request_id, "clipboard_error", &message, Some(status));
        }
    };
    let final_transcript = Transcript {
        text: snippet_text.clone(),
        language: None,
        duration_ms: Some(recording.duration_ms),
        segments: Vec::new(),
    };
    let result = DictationResult {
        job_id: recording.job_id.clone(),
        transcript: final_transcript.clone(),
        benchmark: benchmark.clone(),
        total_ms: elapsed_ms(started),
        copied_to_clipboard: delivery.copied_to_clipboard,
        pasted: delivery.paste_outcome.paste_succeeded,
        paste_attempted: delivery.paste_outcome.paste_attempted,
        paste_succeeded: delivery.paste_outcome.paste_succeeded,
        clipboard_restored: delivery.clipboard_restored,
        cleanup_used: false,
        cleanup_failed: false,
        snippet_used: Some(name),
        insertion_reason: delivery.paste_outcome.insertion_reason.clone(),
    };
    let _ = state.events.send(Event::Result {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        result,
    });
    if state.notifications.enabled {
        voxline_platform::notify(
            "VoxLine",
            if delivery.paste_outcome.paste_succeeded {
                "Snippet paste command sent"
            } else if delivery.copied_to_clipboard {
                "Snippet copied to clipboard"
            } else {
                "Snippet ready"
            },
        );
    }
    update_state(state, Some(recording.job_id.clone()), JobState::Done).await;
    let status = update_state(state, None, JobState::Idle).await;
    data_response(
        request_id,
        status,
        Some(recording),
        Some(final_transcript),
        Some(benchmark),
    )
}

async fn insert_snippet(request_id: String, state: &AppState, name: String) -> Response {
    if state.status.read().await.job_state != JobState::Idle {
        return state_error(request_id, state, "busy", "VoxLine is busy").await;
    }
    let started = Instant::now();
    let job_id = JobId::new();
    let target_at_insert = voxline_platform::capture_active_target();
    *state.target_at_start.lock().await = target_at_insert.clone();
    let paths_config = Config::load_or_default()
        .map(|config| config.paths)
        .unwrap_or_default();
    let prefer_clipboard_only =
        prefer_clipboard_for_target(target_at_insert.as_ref(), &paths_config);
    let content = match voxline_core::snippets::load_snippet_content(&paths_config, &name) {
        Ok(content) => content,
        Err(error) => {
            return state_error(request_id, state, "snippet_error", &error.to_string()).await;
        }
    };
    let delivery = match deliver_text_to_target(
        state,
        &job_id,
        &content,
        target_at_insert,
        started,
        prefer_clipboard_only,
    )
    .await
    {
        Ok(delivery) => delivery,
        Err(message) => {
            let status = update_state(state, None, JobState::Idle).await;
            emit_error(state, Some(job_id), "clipboard_error", &message);
            return error_response(request_id, "clipboard_error", &message, Some(status));
        }
    };
    let copied_to_clipboard = delivery.copied_to_clipboard;
    let paste_outcome = delivery.paste_outcome;
    let clipboard_restored = delivery.clipboard_restored;
    let transcript = Transcript {
        text: content.clone(),
        language: None,
        duration_ms: None,
        segments: Vec::new(),
    };
    let result = DictationResult {
        job_id: job_id.clone(),
        transcript: transcript.clone(),
        benchmark: AsrBenchmark {
            model_load_ms: 0,
            transcribe_ms: 0,
            audio_duration_ms: 0,
        },
        total_ms: elapsed_ms(started),
        copied_to_clipboard,
        pasted: paste_outcome.paste_succeeded,
        paste_attempted: paste_outcome.paste_attempted,
        paste_succeeded: paste_outcome.paste_succeeded,
        clipboard_restored,
        cleanup_used: false,
        cleanup_failed: false,
        snippet_used: Some(name),
        insertion_reason: paste_outcome.insertion_reason.clone(),
    };
    let _ = state.events.send(Event::Result {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        result,
    });
    if state.notifications.enabled {
        voxline_platform::notify(
            "VoxLine",
            if paste_outcome.paste_succeeded {
                "Snippet paste command sent"
            } else if copied_to_clipboard {
                "Snippet copied to clipboard"
            } else {
                "Snippet ready"
            },
        );
    }
    update_state(state, Some(job_id.clone()), JobState::Done).await;
    let status = update_state(state, None, JobState::Idle).await;
    data_response(request_id, status, None, Some(transcript), None)
}

struct DeliveredText {
    copied_to_clipboard: bool,
    paste_outcome: injection::PasteOutcome,
    clipboard_restored: bool,
}

fn prefer_clipboard_for_target(
    target: Option<&voxline_platform::TargetContext>,
    paths: &PathsConfig,
) -> bool {
    target
        .and_then(|target| {
            voxline_core::apps::match_app_profile(
                paths,
                target.app_id.as_deref(),
                target.title.as_deref(),
            )
        })
        .and_then(|profile| profile.injection.prefer_clipboard_only)
        .unwrap_or(false)
}

async fn deliver_text_to_target(
    state: &AppState,
    job_id: &JobId,
    text: &str,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    prefer_clipboard_only: bool,
) -> Result<DeliveredText, String> {
    let clipboard_snapshot = state
        .injection
        .restore_clipboard
        .then(voxline_platform::save_clipboard);
    let copied_to_clipboard = copy_final_text(state, job_id, text).await?;
    let paste_outcome = if copied_to_clipboard {
        insert_if_safe(
            state,
            job_id,
            target_at_stop,
            started,
            prefer_clipboard_only,
        )
        .await
    } else {
        injection::PasteOutcome::disabled("clipboard output is disabled")
    };
    let clipboard_restored = if injection::should_restore_clipboard(
        state.injection.restore_clipboard,
        paste_outcome.paste_succeeded,
    ) && let Some(snapshot) = clipboard_snapshot
    {
        voxline_platform::wait_for_clipboard(state.injection.paste_delay_ms);
        match voxline_platform::restore_clipboard(snapshot) {
            Ok(()) => true,
            Err(error) => {
                warn!(%error, "failed to restore previous clipboard");
                false
            }
        }
    } else {
        false
    };
    Ok(DeliveredText {
        copied_to_clipboard,
        paste_outcome,
        clipboard_restored,
    })
}

async fn copy_final_text(state: &AppState, job_id: &JobId, text: &str) -> Result<bool, String> {
    if !state.injection.copy_to_clipboard {
        return Ok(false);
    }
    update_state(state, Some(job_id.clone()), JobState::Copying).await;
    voxline_platform::copy_to_clipboard(text).map_err(|error| {
        let message = error.to_string();
        if state.notifications.enabled {
            voxline_platform::notify("VoxLine clipboard failed", &message);
        }
        message
    })?;
    Ok(true)
}

async fn insert_if_safe(
    state: &AppState,
    job_id: &JobId,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    prefer_clipboard_only: bool,
) -> injection::PasteOutcome {
    if prefer_clipboard_only {
        return handle_clipboard_fallback(
            state,
            job_id,
            injection::PasteOutcome::clipboard_only(
                "application profile prefers clipboard-only output",
                "paste_profile_clipboard_only",
            ),
        );
    }
    let target_at_start = state.target_at_start.lock().await.take();
    let target_before_paste = voxline_platform::capture_active_target();
    let paste_backend = voxline_platform::paste_backend();
    if let Some(outcome) = injection::evaluate_paste_safety(
        &state.injection.auto_paste,
        paste_backend,
        target_at_start.as_ref(),
        target_at_stop.as_ref(),
        target_before_paste.as_ref(),
        elapsed_ms(started),
        state.injection.max_paste_age_ms,
    ) {
        return handle_clipboard_fallback(state, job_id, outcome);
    }
    update_state(state, Some(job_id.clone()), JobState::Injecting).await;
    voxline_platform::wait_for_clipboard(state.injection.paste_delay_ms);
    match voxline_platform::paste(paste_backend.expect("safety check passed")) {
        Ok(()) => injection::PasteOutcome::succeeded(),
        Err(error) => handle_clipboard_fallback(
            state,
            job_id,
            injection::PasteOutcome::failed_after_attempt(format!("paste failed: {error}")),
        ),
    }
}

fn handle_clipboard_fallback(
    state: &AppState,
    job_id: &JobId,
    outcome: injection::PasteOutcome,
) -> injection::PasteOutcome {
    if injection::should_emit_clipboard_fallback_error(
        state.injection.fallback_to_clipboard_only,
        outcome.warning_code,
    ) {
        emit_error(
            state,
            Some(job_id.clone()),
            outcome.warning_code.expect("warning code checked"),
            &outcome.insertion_reason,
        );
    }
    if state.notifications.enabled
        && injection::should_notify_clipboard_only(
            state.injection.fallback_to_clipboard_only,
            state.injection.notify_on_clipboard_only,
            outcome.warning_code,
        )
    {
        voxline_platform::notify("VoxLine clipboard only", &outcome.insertion_reason);
    }
    outcome
}

async fn test_clipboard(request_id: String, state: &AppState) -> Response {
    let snapshot = voxline_platform::save_clipboard();
    let test_value = format!("VoxLine clipboard test {}", now_ms());
    let result = voxline_platform::copy_to_clipboard(&test_value)
        .and_then(|()| voxline_platform::read_clipboard())
        .and_then(|value| {
            if value == test_value {
                Ok(())
            } else {
                Err(voxline_platform::PlatformError::InvalidOutput {
                    tool: "clipboard",
                    message: "clipboard contents did not match".into(),
                })
            }
        });
    let restore_result = voxline_platform::restore_clipboard(snapshot);
    match result.and(restore_result) {
        Ok(()) => ok_response(request_id, state.status.read().await.clone()),
        Err(error) => {
            state_error(
                request_id,
                state,
                "clipboard_test_failed",
                &error.to_string(),
            )
            .await
        }
    }
}

async fn test_paste(request_id: String, state: &AppState) -> Response {
    let Some(target) = voxline_platform::capture_active_target() else {
        return state_error(
            request_id,
            state,
            "paste_test_unavailable",
            "active target detection is unavailable",
        )
        .await;
    };
    let Some(backend) = voxline_platform::paste_backend() else {
        return state_error(
            request_id,
            state,
            "paste_test_unavailable",
            "no supported paste adapter is available",
        )
        .await;
    };
    if backend != voxline_platform::PasteBackend::Hyprland && target.is_terminal() {
        return state_error(
            request_id,
            state,
            "paste_test_unavailable",
            "terminal paste shortcuts vary; test paste in a graphical text field",
        )
        .await;
    }
    let snapshot = voxline_platform::save_clipboard();
    if let Err(error) = voxline_platform::copy_to_clipboard("VoxLine paste test") {
        return state_error(request_id, state, "paste_test_failed", &error.to_string()).await;
    }
    voxline_platform::wait_for_clipboard(state.injection.paste_delay_ms);
    if voxline_platform::capture_active_target().as_ref() != Some(&target) {
        return state_error(
            request_id,
            state,
            "paste_test_failed",
            "active target changed before paste",
        )
        .await;
    }
    let result = voxline_platform::paste(backend);
    voxline_platform::wait_for_clipboard(state.injection.paste_delay_ms);
    let restore_result = voxline_platform::restore_clipboard(snapshot);
    match result.and(restore_result) {
        Ok(()) => ok_response(request_id, state.status.read().await.clone()),
        Err(error) => state_error(request_id, state, "paste_test_failed", &error.to_string()).await,
    }
}

struct TemporaryAudio {
    path: std::path::PathBuf,
    retain: bool,
}

impl TemporaryAudio {
    fn new(path: std::path::PathBuf, retain: bool) -> Self {
        Self { path, retain }
    }
}

impl Drop for TemporaryAudio {
    fn drop(&mut self) {
        if self.retain {
            return;
        }
        if let Err(error) = fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            warn!(path = %self.path.display(), %error, "failed to delete temporary audio");
        }
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
    state.preview.stop().await;
    if state.preview.is_enabled() {
        update_preview_model_state(state, ModelState::Unloaded).await;
    }
    match state.audio.cancel(job_id.clone()).await {
        Ok(()) => {
            *state.target_at_start.lock().await = None;
            *state.cleanup_override.lock().await = None;
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

async fn stream_subscribe(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    state: &AppState,
    kinds: Vec<EventKind>,
) -> Result<()> {
    let want_preview = kinds.contains(&EventKind::Preview);
    let want_other = kinds.iter().any(|kind| *kind != EventKind::Preview);
    if !want_preview {
        return stream_events(writer, state.events.subscribe(), &kinds).await;
    }

    let mut events_rx = state.events.subscribe();
    let mut preview_rx = state.preview.subscribe();
    let _ = preview_rx.borrow_and_update();

    loop {
        if want_other && want_preview {
            tokio::select! {
                event = events_rx.recv() => match event {
                    Ok(event) if event_matches(&event, &kinds) => {
                        write_json_line(writer, &event).await?;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        anyhow::bail!("event subscriber is too slow");
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                },
                changed = preview_rx.changed() => {
                    if changed.is_err() {
                        return Ok(());
                    }
                    let snapshot = {
                        let guard = preview_rx.borrow();
                        guard.clone()
                    };
                    if let Some(snapshot) = snapshot {
                        write_json_line(
                            writer,
                            &Event::Preview {
                                protocol_version: PROTOCOL_VERSION,
                                timestamp_ms: now_ms(),
                                job_id: snapshot.job_id,
                                stable: snapshot.stable,
                                provisional: snapshot.provisional,
                                speech_active: snapshot.speech_active,
                            },
                        )
                        .await?;
                    }
                }
            }
        } else {
            if preview_rx.changed().await.is_err() {
                return Ok(());
            }
            let snapshot = {
                let guard = preview_rx.borrow();
                guard.clone()
            };
            if let Some(snapshot) = snapshot {
                write_json_line(
                    writer,
                    &Event::Preview {
                        protocol_version: PROTOCOL_VERSION,
                        timestamp_ms: now_ms(),
                        job_id: snapshot.job_id,
                        stable: snapshot.stable,
                        provisional: snapshot.provisional,
                        speech_active: snapshot.speech_active,
                    },
                )
                .await?;
            }
        }
    }
}

fn event_matches(event: &Event, kinds: &[EventKind]) -> bool {
    kinds.iter().any(|kind| {
        matches!(
            (kind, event),
            (EventKind::State, Event::State { .. })
                | (EventKind::Result, Event::Result { .. })
                | (EventKind::Error, Event::Error { .. })
                | (EventKind::Preview, Event::Preview { .. })
        )
    })
}

async fn stream_events(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    mut receiver: broadcast::Receiver<Event>,
    kinds: &[EventKind],
) -> Result<()> {
    loop {
        match receiver.recv().await {
            Ok(event) if event_matches(&event, kinds) => {
                write_json_line(writer, &event).await?;
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => {
                anyhow::bail!("event subscriber is too slow");
            }
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

async fn update_preview_model_state(state: &AppState, model_state: ModelState) {
    let mut status = state.status.write().await;
    if status.preview_model_state.is_none() {
        return;
    }
    status.preview_model_state = Some(model_state);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}
