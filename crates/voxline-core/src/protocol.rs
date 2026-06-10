use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

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
    Toggle,
    Start,
    Stop,
    Cancel,
    Subscribe { events: Vec<EventKind> },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    State,
    Result,
    Error,
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
    pub error: Option<ProtocolError>,
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
}
