use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, RwLock, broadcast};
use voxline_core::{
    cleanup::CleanupOverride,
    config::{
        AudioGatesConfig, CleanupConfig, Config, InjectionConfig, NotificationsConfig, PathsConfig,
        PrivacyConfig, SecretsConfig, VoiceCommandsConfig,
    },
    protocol::{
        AsrBenchmark, AudioRecording, DaemonStatus, DictationResult, Event, JobId, JobState,
        ModelState, PROTOCOL_VERSION, ProtocolError, Response, SessionEnvironment, Transcript,
    },
};

use crate::{asr, audio, preview, preview_asr};

#[derive(Clone)]
pub(crate) struct JobConfigSnapshot {
    pub(crate) cleanup: CleanupConfig,
    pub(crate) paths: PathsConfig,
    pub(crate) secrets: SecretsConfig,
    pub(crate) cleanup_enabled: bool,
    pub(crate) voice_commands: VoiceCommandsConfig,
    pub(crate) preview_ring_buffer_seconds: u64,
}

pub(crate) struct AppState {
    pub(crate) status: RwLock<DaemonStatus>,
    pub(crate) events: broadcast::Sender<Event>,
    pub(crate) preview: preview::PreviewCoordinator,
    pub(crate) preview_asr: Option<preview_asr::PreviewAsrManager>,
    pub(crate) audio: audio::AudioRecorder,
    pub(crate) asr: asr::AsrManager,
    pub(crate) audio_gates: AudioGatesConfig,
    pub(crate) injection: InjectionConfig,
    pub(crate) notifications: NotificationsConfig,
    pub(crate) privacy: PrivacyConfig,
    pub(crate) target_at_start: Mutex<Option<voxline_platform::TargetContext>>,
    pub(crate) cleanup_override: Mutex<Option<CleanupOverride>>,
    pub(crate) style_override: Mutex<Option<String>>,
    pub(crate) active_app_profile: Mutex<Option<voxline_core::apps::AppProfile>>,
    pub(crate) job_config: Mutex<Option<JobConfigSnapshot>>,
}
pub(crate) fn snapshot_job_config(config: &Config) -> JobConfigSnapshot {
    JobConfigSnapshot {
        cleanup: config.cleanup.clone(),
        paths: config.paths.clone(),
        secrets: config.secrets.clone(),
        cleanup_enabled: config.cleanup.enabled,
        voice_commands: config.voice_commands.clone(),
        preview_ring_buffer_seconds: config.preview.ring_buffer_seconds,
    }
}

pub(crate) fn load_job_config_snapshot() -> JobConfigSnapshot {
    Config::load_or_default().map_or_else(
        |_| JobConfigSnapshot {
            cleanup: CleanupConfig::default(),
            paths: PathsConfig::default(),
            secrets: SecretsConfig::default(),
            cleanup_enabled: false,
            voice_commands: VoiceCommandsConfig::default(),
            preview_ring_buffer_seconds: 30,
        },
        |config| snapshot_job_config(&config),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BeginJobKind {
    Recording,
    Transcribing,
    Copying,
}

fn begin_job_kind_for(state: &JobState) -> BeginJobKind {
    match *state {
        JobState::Recording => BeginJobKind::Recording,
        JobState::Copying => BeginJobKind::Copying,
        _ => BeginJobKind::Transcribing,
    }
}

#[must_use]
pub(crate) fn begin_job_allowed(current: &JobState, _kind: BeginJobKind) -> bool {
    *current == JobState::Idle
}

pub(crate) async fn try_begin_job(
    state: &AppState,
    job_id: Option<JobId>,
    new_state: JobState,
) -> Result<DaemonStatus, ()> {
    let mut status = state.status.write().await;
    if !begin_job_allowed(&status.job_state, begin_job_kind_for(&new_state)) {
        return Err(());
    }
    status.active_job_id.clone_from(&job_id);
    status.job_state.clone_from(&new_state);
    let snapshot = status.clone();
    let _ = state.events.send(Event::State {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        job_id,
        job_state: new_state,
        final_model_state: snapshot.final_model_state.clone(),
    });
    Ok(snapshot)
}

pub(crate) async fn clear_per_job_context(state: &AppState) {
    *state.cleanup_override.lock().await = None;
    *state.style_override.lock().await = None;
    *state.target_at_start.lock().await = None;
    *state.active_app_profile.lock().await = None;
    *state.job_config.lock().await = None;
}
pub(crate) async fn update_state(
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

pub(crate) async fn state_error(
    request_id: String,
    state: &AppState,
    code: &str,
    message: &str,
) -> Response {
    let status = state.status.read().await.clone();
    emit_error(state, status.active_job_id.clone(), code, message);
    error_response(request_id, code, message, Some(status))
}

pub(crate) async fn audio_error_response(
    request_id: String,
    state: &AppState,
    error: audio::AudioError,
) -> Response {
    if state.notifications.enabled && matches!(&error, audio::AudioError::StreamFailed { .. }) {
        voxline_platform::notify("VoxLine", "Recording failed: audio stream error");
    }
    emit_error(state, None, "audio_error", &error.to_string());
    // `JobState::Failed` is reserved in the protocol but unused; pipeline errors reset to Idle.
    clear_per_job_context(state).await;
    let status = update_state(state, None, JobState::Idle).await;
    error_response(request_id, "audio_error", &error.to_string(), Some(status))
}

pub(crate) fn emit_error(state: &AppState, job_id: Option<JobId>, code: &str, message: &str) {
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
pub(crate) async fn update_preview_model_state(state: &AppState, model_state: ModelState) {
    let mut status = state.status.write().await;
    if status.preview_model_state.is_none() {
        return;
    }
    status.preview_model_state = Some(model_state);
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

pub(crate) fn daemon_environment_response(request_id: String, status: DaemonStatus) -> Response {
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

pub(crate) fn current_session_environment() -> SessionEnvironment {
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

pub(crate) fn ok_response(request_id: String, status: DaemonStatus) -> Response {
    success_response(request_id, status, None)
}

pub(crate) fn success_response(
    request_id: String,
    status: DaemonStatus,
    recording: Option<AudioRecording>,
) -> Response {
    data_response(request_id, status, recording, None, None)
}

pub(crate) fn data_response(
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

pub(crate) fn data_response_with(
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

pub(crate) fn error_response(
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

pub(crate) fn reload_job_config() -> (
    CleanupConfig,
    PathsConfig,
    SecretsConfig,
    bool,
    VoiceCommandsConfig,
) {
    let snapshot = load_job_config_snapshot();
    (
        snapshot.cleanup,
        snapshot.paths,
        snapshot.secrets,
        snapshot.cleanup_enabled,
        snapshot.voice_commands,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopDecision {
    Allowed,
    NoActiveRecording,
}

#[must_use]
pub(crate) fn stop_decision(current: &JobState) -> StopDecision {
    if *current == JobState::Recording {
        StopDecision::Allowed
    } else {
        StopDecision::NoActiveRecording
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelDecision {
    Allowed,
    NoActiveRecording,
    CannotCancel,
}

#[must_use]
pub(crate) fn cancel_decision(current: &JobState) -> CancelDecision {
    match *current {
        JobState::Idle => CancelDecision::NoActiveRecording,
        JobState::Recording => CancelDecision::Allowed,
        _ => CancelDecision::CannotCancel,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToggleDecision {
    Start,
    Stop,
    Busy,
}

#[must_use]
pub(crate) fn toggle_decision(current: &JobState) -> ToggleDecision {
    match *current {
        JobState::Idle => ToggleDecision::Start,
        JobState::Recording => ToggleDecision::Stop,
        _ => ToggleDecision::Busy,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use voxline_core::config::{AsrConfig, AudioConfig, PreviewConfig, VocabularyConfig};

    fn test_app_state() -> Arc<AppState> {
        let (events, _) = broadcast::channel(32);
        Arc::new(AppState {
            status: RwLock::new(DaemonStatus::default()),
            events,
            preview: preview::PreviewCoordinator::new(PreviewConfig::default()),
            preview_asr: None,
            audio: audio::AudioRecorder::spawn(AudioConfig::default(), PathsConfig::default()),
            asr: asr::AsrManager::spawn(AsrConfig::default(), VocabularyConfig::default()),
            audio_gates: AudioGatesConfig::default(),
            injection: InjectionConfig::default(),
            notifications: NotificationsConfig::default(),
            privacy: PrivacyConfig::default(),
            target_at_start: Mutex::new(None),
            cleanup_override: Mutex::new(None),
            style_override: Mutex::new(None),
            active_app_profile: Mutex::new(None),
            job_config: Mutex::new(None),
        })
    }

    #[tokio::test]
    async fn try_begin_job_race_exactly_one_wins() {
        let state = test_app_state();
        let (first, second) = tokio::join!(
            try_begin_job(&state, Some(JobId::new()), JobState::Recording),
            try_begin_job(&state, Some(JobId::new()), JobState::Recording),
        );
        let winners = usize::from(first.is_ok()) + usize::from(second.is_ok());
        assert_eq!(winners, 1);
        assert_eq!(state.status.read().await.job_state, JobState::Recording);
    }

    #[test]
    fn busy_matrix_begin_job() {
        let states = [
            JobState::Idle,
            JobState::Recording,
            JobState::Transcribing,
            JobState::Cleaning,
            JobState::Copying,
        ];
        let kinds = [
            BeginJobKind::Recording,
            BeginJobKind::Transcribing,
            BeginJobKind::Copying,
        ];
        for state in states {
            for kind in kinds {
                let allowed = begin_job_allowed(&state, kind);
                assert_eq!(
                    allowed,
                    state == JobState::Idle,
                    "begin {kind:?} in {state:?}"
                );
            }
        }
    }

    #[test]
    fn busy_matrix_stop() {
        let cases = [
            (JobState::Idle, StopDecision::NoActiveRecording),
            (JobState::Recording, StopDecision::Allowed),
            (JobState::Transcribing, StopDecision::NoActiveRecording),
            (JobState::Cleaning, StopDecision::NoActiveRecording),
            (JobState::Copying, StopDecision::NoActiveRecording),
        ];
        for (state, expected) in cases {
            assert_eq!(stop_decision(&state), expected, "stop in {state:?}");
        }
    }

    #[test]
    fn busy_matrix_cancel() {
        let cases = [
            (JobState::Idle, CancelDecision::NoActiveRecording),
            (JobState::Recording, CancelDecision::Allowed),
            (JobState::Transcribing, CancelDecision::CannotCancel),
            (JobState::Cleaning, CancelDecision::CannotCancel),
            (JobState::Copying, CancelDecision::CannotCancel),
        ];
        for (state, expected) in cases {
            assert_eq!(cancel_decision(&state), expected, "cancel in {state:?}");
        }
    }

    #[test]
    fn busy_matrix_toggle() {
        let cases = [
            (JobState::Idle, ToggleDecision::Start),
            (JobState::Recording, ToggleDecision::Stop),
            (JobState::Transcribing, ToggleDecision::Busy),
            (JobState::Cleaning, ToggleDecision::Busy),
            (JobState::Copying, ToggleDecision::Busy),
        ];
        for (state, expected) in cases {
            assert_eq!(toggle_decision(&state), expected, "toggle in {state:?}");
        }
    }
}
