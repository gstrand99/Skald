use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixStream, unix::OwnedReadHalf},
};
use ulid::Ulid;

use crate::{
    config::{Config, PathsConfig},
    protocol::{Command, Event, EventKind, PROTOCOL_VERSION, Request, Response},
    runtime::socket_path_for,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolVersionError {
    #[error(
        "daemon protocol version {daemon_version} is newer than client version {client_version}"
    )]
    DaemonNewer {
        daemon_version: u32,
        client_version: u32,
    },
}

/// Returns `Err` when the peer advertises a protocol version newer than this client.
pub fn ensure_supported_protocol_version(version: u32) -> Result<(), ProtocolVersionError> {
    if version > PROTOCOL_VERSION {
        Err(ProtocolVersionError::DaemonNewer {
            daemon_version: version,
            client_version: PROTOCOL_VERSION,
        })
    } else {
        Ok(())
    }
}

#[must_use]
pub fn protocol_version_from_event(event: &Event) -> u32 {
    match event {
        Event::State {
            protocol_version, ..
        }
        | Event::Error {
            protocol_version, ..
        }
        | Event::Result {
            protocol_version, ..
        }
        | Event::Preview {
            protocol_version, ..
        } => *protocol_version,
    }
}

pub fn socket_path_from_config() -> Result<PathBuf> {
    let config = Config::load_or_default()?;
    socket_path_for(&config.paths).context("could not resolve daemon socket path")
}

pub fn socket_path_for_paths(paths: &PathsConfig) -> Result<PathBuf> {
    socket_path_for(paths).context("could not resolve daemon socket path")
}

#[must_use]
pub fn overlay_event_kinds() -> Vec<EventKind> {
    vec![
        EventKind::State,
        EventKind::Preview,
        EventKind::Result,
        EventKind::Error,
    ]
}

pub async fn connect_socket(path: &Path) -> Result<UnixStream> {
    UnixStream::connect(path)
        .await
        .with_context(|| format!("cannot connect to {}; is voxlined running?", path.display()))
}

pub async fn subscribe(path: &Path, events: Vec<EventKind>) -> Result<(Response, OwnedReadHalf)> {
    let stream = connect_socket(path).await?;
    let (reader, mut writer) = stream.into_split();
    let request = Request {
        protocol_version: PROTOCOL_VERSION,
        request_id: Ulid::new().to_string(),
        command: Command::Subscribe { events },
    };
    write_request(&mut writer, &request).await?;
    let mut lines = BufReader::new(reader).lines();
    let line = lines
        .next_line()
        .await?
        .context("daemon closed without a subscribe response")?;
    let response: Response = serde_json::from_str(&line)?;
    ensure_supported_protocol_version(response.protocol_version)?;
    let reader = lines.into_inner().into_inner();
    Ok((response, reader))
}

pub async fn read_event(reader: &mut BufReader<OwnedReadHalf>) -> Result<Event> {
    let mut line = String::new();
    let nbytes = reader.read_line(&mut line).await?;
    if nbytes == 0 {
        anyhow::bail!("daemon closed the event stream");
    }
    let event: Event =
        serde_json::from_str(line.trim_end()).context("failed to parse daemon event")?;
    ensure_supported_protocol_version(protocol_version_from_event(&event))?;
    Ok(event)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{JobId, JobState, ModelState, PublicDictationResult};

    #[test]
    fn accepts_current_and_older_protocol_versions() {
        ensure_supported_protocol_version(PROTOCOL_VERSION).expect("current version");
        ensure_supported_protocol_version(PROTOCOL_VERSION.saturating_sub(1))
            .expect("older version");
    }

    #[test]
    fn rejects_newer_daemon_protocol_versions() {
        let error = ensure_supported_protocol_version(PROTOCOL_VERSION + 1)
            .expect_err("newer daemon version");
        assert_eq!(
            error,
            ProtocolVersionError::DaemonNewer {
                daemon_version: PROTOCOL_VERSION + 1,
                client_version: PROTOCOL_VERSION,
            }
        );
    }

    #[test]
    fn reads_protocol_version_from_each_event_variant() {
        let state = Event::State {
            protocol_version: 7,
            timestamp_ms: 0,
            job_id: None,
            job_state: JobState::Idle,
            final_model_state: ModelState::Unloaded,
        };
        assert_eq!(protocol_version_from_event(&state), 7);

        let preview = Event::Preview {
            protocol_version: 8,
            timestamp_ms: 0,
            job_id: JobId::new(),
            stable: String::new(),
            provisional: String::new(),
            speech_active: false,
        };
        assert_eq!(protocol_version_from_event(&preview), 8);

        let result = Event::Result {
            protocol_version: 9,
            timestamp_ms: 0,
            result: PublicDictationResult {
                job_id: JobId::new(),
                total_ms: 0,
                copied_to_clipboard: false,
                paste_attempted: false,
                paste_succeeded: false,
                clipboard_restored: false,
                cleanup_used: false,
                cleanup_failed: false,
                snippet_used: None,
                insertion_reason: String::new(),
                transcript: None,
            },
        };
        assert_eq!(protocol_version_from_event(&result), 9);
    }
}
