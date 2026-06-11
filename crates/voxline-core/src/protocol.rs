use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::cleanup::CleanupOverride;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub Ulid);

impl JobId {
    #[must_use]
    pub fn new() -> Self {
        Self(Ulid::new())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Status,
    Toggle {
        #[serde(default)]
        cleanup: Option<CleanupOverride>,
        #[serde(default)]
        style: Option<String>,
        #[serde(default)]
        snippet: Option<String>,
    },
    InsertSnippet {
        name: String,
    },
    TemplatePreview {
        name: String,
        text: String,
    },
    Start,
    Stop,
    Cancel,
    Transcribe {
        audio_path: PathBuf,
    },
    BenchDictation {
        audio_path: PathBuf,
        #[serde(default)]
        cleanup: Option<CleanupOverride>,
        #[serde(default = "default_true")]
        attempt_paste: bool,
    },
    SetupRecord {
        seconds: u64,
        output_path: PathBuf,
    },
    BenchModelCompare {
        audio_path: PathBuf,
        candidates: Vec<AsrBenchCandidate>,
        #[serde(default = "default_true")]
        include_cold_load: bool,
    },
    AsrStatus,
    AsrLoad,
    AsrUnload,
    AsrRestart,
    TestClipboard,
    TestPaste,
    TestOpenrouter,
    CleanupPreview {
        text: String,
        #[serde(default)]
        style: Option<String>,
    },
    DaemonEnvironment,
    Subscribe {
        events: Vec<EventKind>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct SessionEnvironment {
    pub session_type: Option<String>,
    pub desktop: Option<String>,
    pub wayland_display_present: bool,
    pub display_present: bool,
    pub dbus_session_bus_present: bool,
    pub xdg_runtime_dir_present: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    State,
    Result,
    Error,
    Preview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub protocol_version: u32,
    pub request_id: String,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub protocol_version: u32,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<DaemonStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<AudioRecording>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Transcript>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub benchmark: Option<AsrBenchmark>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_environment: Option<SessionEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleaned_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dictation: Option<DictationResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_bench_results: Option<Vec<ModelBenchResult>>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRecording {
    pub job_id: JobId,
    pub wav_path: PathBuf,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub rms_energy: f32,
    pub peak_energy: f32,
    pub speech_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
    pub duration_ms: Option<u64>,
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrBenchmark {
    pub model_load_ms: u64,
    pub transcribe_ms: u64,
    pub audio_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AsrBenchCandidate {
    pub model_id: String,
    pub model_path: PathBuf,
    pub gpu: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelBenchResult {
    pub model_id: String,
    pub cold_load_ms: u64,
    pub warm_transcribe_ms: u64,
    pub audio_duration_ms: u64,
    pub transcript_text: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct DictationResult {
    pub job_id: JobId,
    pub transcript: Transcript,
    pub benchmark: AsrBenchmark,
    pub total_ms: u64,
    pub copied_to_clipboard: bool,
    pub pasted: bool,
    pub paste_attempted: bool,
    pub paste_succeeded: bool,
    pub clipboard_restored: bool,
    pub cleanup_used: bool,
    pub cleanup_failed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet_used: Option<String>,
    pub insertion_reason: String,
}

/// Metadata-only dictation outcome for broadcast events. Transcript text is omitted
/// unless explicitly enabled via privacy config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct PublicDictationResult {
    pub job_id: JobId,
    pub total_ms: u64,
    pub copied_to_clipboard: bool,
    pub paste_attempted: bool,
    pub paste_succeeded: bool,
    pub clipboard_restored: bool,
    pub cleanup_used: bool,
    pub cleanup_failed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet_used: Option<String>,
    pub insertion_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Transcript>,
}

impl PublicDictationResult {
    #[must_use]
    pub fn from_result(result: &DictationResult, include_transcript: bool) -> Self {
        Self {
            job_id: result.job_id.clone(),
            total_ms: result.total_ms,
            copied_to_clipboard: result.copied_to_clipboard,
            paste_attempted: result.paste_attempted,
            paste_succeeded: result.paste_succeeded,
            clipboard_restored: result.clipboard_restored,
            cleanup_used: result.cleanup_used,
            cleanup_failed: result.cleanup_failed,
            snippet_used: result.snippet_used.clone(),
            insertion_reason: result.insertion_reason.clone(),
            transcript: include_transcript.then(|| result.transcript.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobState {
    Idle,
    Recording,
    Stopping,
    Transcribing,
    Cleaning,
    Copying,
    Injecting,
    Done,
    Cancelled,
    Failed { code: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModelState {
    Unloaded,
    Loading,
    Ready,
    Failed { code: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub protocol_version: u32,
    pub active_job_id: Option<JobId>,
    pub job_state: JobState,
    pub final_model_state: ModelState,
    pub preview_model_state: Option<ModelState>,
    pub cleanup_enabled: bool,
    pub auto_paste_effective: String,
    pub asr_gpu_build: bool,
}

impl Default for DaemonStatus {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            active_job_id: None,
            job_state: JobState::Idle,
            final_model_state: ModelState::Unloaded,
            preview_model_state: None,
            cleanup_enabled: false,
            auto_paste_effective: "clipboard_only".into(),
            asr_gpu_build: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    State {
        protocol_version: u32,
        timestamp_ms: u64,
        job_id: Option<JobId>,
        job_state: JobState,
        final_model_state: ModelState,
    },
    Error {
        protocol_version: u32,
        timestamp_ms: u64,
        job_id: Option<JobId>,
        error: ProtocolError,
    },
    Result {
        protocol_version: u32,
        timestamp_ms: u64,
        result: PublicDictationResult,
    },
    Preview {
        protocol_version: u32,
        timestamp_ms: u64,
        job_id: JobId,
        stable: String,
        provisional: String,
        speech_active: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_status_request_as_ndjson_payload() {
        let request = Request {
            protocol_version: 1,
            request_id: "r1".into(),
            command: Command::Status,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"cmd\":\"status\""));
    }

    #[test]
    fn serializes_toggle_with_snippet() {
        let request = Request {
            protocol_version: 1,
            request_id: "r2".into(),
            command: Command::Toggle {
                cleanup: None,
                style: None,
                snippet: Some("signature".into()),
            },
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"cmd\":\"toggle\""));
        assert!(json.contains("\"snippet\":\"signature\""));
    }

    #[test]
    fn public_dictation_result_omits_transcript_by_default() {
        let result = DictationResult {
            job_id: JobId::new(),
            transcript: Transcript {
                text: "secret dictated text".into(),
                language: None,
                duration_ms: None,
                segments: vec![],
            },
            benchmark: AsrBenchmark {
                model_load_ms: 0,
                transcribe_ms: 0,
                audio_duration_ms: 0,
            },
            total_ms: 42,
            copied_to_clipboard: true,
            pasted: false,
            paste_attempted: false,
            paste_succeeded: false,
            clipboard_restored: false,
            cleanup_used: false,
            cleanup_failed: false,
            snippet_used: None,
            insertion_reason: "clipboard_only".into(),
        };
        let public = PublicDictationResult::from_result(&result, false);
        let json = serde_json::to_string(&public).unwrap();
        assert!(!json.contains("transcript"));
        assert!(!json.contains("secret dictated text"));
    }
}
