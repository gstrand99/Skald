use std::sync::Arc;

use anyhow::Result;
use skald_core::protocol::{
    Command, Event, EventKind, PROTOCOL_VERSION, ProtocolError, Request, Response,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::broadcast,
};
use tracing::{debug, warn};

use crate::{
    bench::{bench_dictation, bench_model_compare, setup_record},
    delivery::{test_clipboard, test_paste},
    dictation::{
        asr_load, asr_restart, asr_unload, cancel, cleanup_preview, insert_snippet, start, stop,
        template_preview, test_openrouter, toggle, transcribe,
    },
    jobs::{AppState, daemon_environment_response, error_response, now_ms, ok_response},
};

pub(crate) fn reject_foreign_peer(stream: &UnixStream) -> bool {
    let Ok(cred) = stream.peer_cred() else {
        warn!("rejected unix connection: peer credentials unavailable");
        return true;
    };
    let peer_uid = cred.uid();
    let daemon_uid = rustix::process::geteuid().as_raw();
    if peer_uid != daemon_uid {
        warn!(
            peer_uid,
            daemon_uid, "rejected unix connection from different user"
        );
        return true;
    }
    false
}
pub(crate) async fn handle_client(stream: UnixStream, state: Arc<AppState>) -> Result<()> {
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

pub(crate) async fn dispatch(request: Request, state: Arc<AppState>) -> Response {
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

async fn write_json_line<T: serde::Serialize>(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    value: &T,
) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    Ok(())
}

pub(crate) async fn stream_subscribe(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    state: &AppState,
    kinds: Vec<EventKind>,
) -> Result<()> {
    let want_preview = kinds.contains(&EventKind::Preview);
    let want_audio_level = kinds.contains(&EventKind::AudioLevel);
    let want_other = kinds
        .iter()
        .any(|kind| !matches!(kind, EventKind::Preview | EventKind::AudioLevel));
    if !want_preview && !want_audio_level {
        return stream_events(writer, state.events.subscribe(), &kinds).await;
    }

    let mut events_rx = state.events.subscribe();
    let mut preview_rx = state.preview.subscribe();
    let mut audio_level_rx = state.audio.subscribe_levels();
    let _ = preview_rx.borrow_and_update();
    let _ = audio_level_rx.borrow_and_update();

    loop {
        tokio::select! {
            event = events_rx.recv(), if want_other => match event {
                Ok(event) if event_matches(&event, &kinds) => {
                    write_json_line(writer, &event).await?;
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    debug!(skipped, "event subscriber lagged; skipping ahead");
                }
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            changed = preview_rx.changed(), if want_preview => {
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
            changed = audio_level_rx.changed(), if want_audio_level => {
                if changed.is_err() {
                    return Ok(());
                }
                let snapshot = {
                    let guard = audio_level_rx.borrow();
                    guard.clone()
                };
                if let Some(snapshot) = snapshot {
                    write_json_line(
                        writer,
                        &Event::AudioLevel {
                            protocol_version: PROTOCOL_VERSION,
                            timestamp_ms: now_ms(),
                            job_id: snapshot.job_id,
                            rms: snapshot.rms,
                            peak: snapshot.peak,
                        },
                    )
                    .await?;
                }
            }
        }
    }
}

pub(crate) fn event_matches(event: &Event, kinds: &[EventKind]) -> bool {
    kinds.iter().any(|kind| {
        matches!(
            (kind, event),
            (EventKind::State, Event::State { .. })
                | (EventKind::Result, Event::Result { .. })
                | (EventKind::Error, Event::Error { .. })
        )
    })
}

pub(crate) async fn stream_events(
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
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                debug!(skipped, "event subscriber lagged; skipping ahead");
            }
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}
