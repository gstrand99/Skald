use serde::Serialize;

use crate::protocol::{DaemonStatus, Event, JobState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesktopStatus {
    pub text: &'static str,
    pub class: &'static str,
    pub tooltip: String,
}

impl DesktopStatus {
    #[must_use]
    pub fn disconnected() -> Self {
        Self {
            text: "󰍭",
            class: "disconnected",
            tooltip: "Skald daemon is disconnected".into(),
        }
    }

    #[must_use]
    pub fn from_daemon(status: &DaemonStatus) -> Self {
        Self::from_job_state(&status.job_state)
    }

    #[must_use]
    pub fn from_event(event: &Event) -> Option<Self> {
        match event {
            Event::State { job_state, .. } => Some(Self::from_job_state(job_state)),
            Event::Error { error, .. } => Some(Self {
                text: "󰍭",
                class: "error",
                tooltip: format!("Skald error: {}", error.message),
            }),
            Event::Result { .. } => Some(Self::from_job_state(&JobState::Idle)),
            Event::Preview { .. } | Event::AudioLevel { .. } => None,
        }
    }

    #[must_use]
    pub fn from_job_state(state: &JobState) -> Self {
        match state {
            JobState::Idle | JobState::Done | JobState::Cancelled => Self {
                text: "󰍬",
                class: "idle",
                tooltip: "Skald is idle".into(),
            },
            JobState::Recording | JobState::Stopping => Self {
                text: "󰑋",
                class: "recording",
                tooltip: "Skald is recording".into(),
            },
            JobState::Transcribing => Self {
                text: "󰔊",
                class: "transcribing",
                tooltip: "Skald is transcribing".into(),
            },
            JobState::Cleaning => Self {
                text: "󰄬",
                class: "cleaning",
                tooltip: "Skald is cleaning transcript text using the configured provider".into(),
            },
            JobState::Copying | JobState::Injecting => Self {
                text: "󰆏",
                class: "transcribing",
                tooltip: "Skald is delivering the result".into(),
            },
            JobState::Failed { message, .. } => Self {
                text: "󰍭",
                class: "error",
                tooltip: format!("Skald error: {message}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Event, PROTOCOL_VERSION, ProtocolError};

    #[test]
    fn exposes_stable_waybar_classes() {
        let cases = [
            (JobState::Idle, "idle"),
            (JobState::Recording, "recording"),
            (JobState::Transcribing, "transcribing"),
            (JobState::Cleaning, "cleaning"),
            (
                JobState::Failed {
                    code: "test".into(),
                    message: "failed".into(),
                },
                "error",
            ),
        ];
        for (state, expected) in cases {
            assert_eq!(DesktopStatus::from_job_state(&state).class, expected);
        }
        assert_eq!(DesktopStatus::disconnected().class, "disconnected");
    }

    #[test]
    fn event_output_never_contains_result_content() {
        let event = Event::Error {
            protocol_version: PROTOCOL_VERSION,
            timestamp_ms: 0,
            job_id: None,
            error: ProtocolError {
                code: "test".into(),
                message: "connection failed".into(),
            },
        };
        let json = serde_json::to_string(&DesktopStatus::from_event(&event).unwrap()).unwrap();
        assert_eq!(
            json,
            r#"{"text":"󰍭","class":"error","tooltip":"Skald error: connection failed"}"#
        );
    }
}
