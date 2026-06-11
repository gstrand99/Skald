use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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
    let reader = lines.into_inner().into_inner();
    Ok((response, reader))
}

pub async fn read_event(reader: &mut BufReader<OwnedReadHalf>) -> Result<Event> {
    let mut line = String::new();
    let nbytes = reader.read_line(&mut line).await?;
    if nbytes == 0 {
        anyhow::bail!("daemon closed the event stream");
    }
    serde_json::from_str(line.trim_end()).context("failed to parse daemon event")
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
