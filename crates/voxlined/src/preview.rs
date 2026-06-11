use std::sync::Arc;
use std::time::Duration;

use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};
use voxline_core::{
    config::PreviewConfig,
    preview::{PreviewAgreement, extract_preview_window, ms_to_samples, window_rms_energy},
    protocol::{Event, JobId, ModelState, PROTOCOL_VERSION},
};

use crate::{asr::AsrError, audio::RecordingTap, preview_asr::PreviewAsrManager};

#[derive(Debug, Clone)]
pub struct PreviewSnapshot {
    pub job_id: JobId,
    pub stable: String,
    pub provisional: String,
    pub speech_active: bool,
}

struct PreviewSession {
    handle: JoinHandle<()>,
}

pub struct PreviewCoordinator {
    config: PreviewConfig,
    session: Mutex<Option<PreviewSession>>,
    updates: watch::Sender<Option<PreviewSnapshot>>,
}

impl PreviewCoordinator {
    #[must_use]
    pub fn new(config: PreviewConfig) -> Self {
        let (updates, _) = watch::channel(None);
        Self {
            config,
            session: Mutex::new(None),
            updates,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<PreviewSnapshot>> {
        self.updates.subscribe()
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub async fn start(
        &self,
        job_id: JobId,
        tap: RecordingTap,
        preview_asr: PreviewAsrManager,
        events: tokio::sync::broadcast::Sender<Event>,
        on_model_state: Arc<dyn Fn(ModelState) + Send + Sync>,
    ) {
        if !self.config.enabled {
            return;
        }
        self.stop().await;
        let config = self.config.clone();
        let updates = self.updates.clone();
        let handle = tokio::spawn(async move {
            run_preview_loop(
                job_id,
                tap,
                preview_asr,
                config,
                updates,
                events,
                on_model_state,
            )
            .await;
        });
        *self.session.lock().await = Some(PreviewSession { handle });
    }

    pub async fn stop(&self) {
        if let Some(session) = self.session.lock().await.take() {
            session.handle.abort();
            let _ = session.handle.await;
        }
        let _ = self.updates.send(None);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_preview_loop(
    job_id: JobId,
    tap: RecordingTap,
    preview_asr: PreviewAsrManager,
    config: PreviewConfig,
    updates: watch::Sender<Option<PreviewSnapshot>>,
    events: tokio::sync::broadcast::Sender<Event>,
    on_model_state: Arc<dyn Fn(ModelState) + Send + Sync>,
) {
    on_model_state(ModelState::Loading);
    let mut step = interval(Duration::from_millis(config.step_ms));
    step.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut agreement = PreviewAgreement::default();
    let mut last_stable = String::new();
    let sample_rate = tap.target_sample_rate();
    let min_window_samples = ms_to_samples(500, sample_rate);
    let mut transcribe_task: Option<JoinHandle<Result<String, AsrError>>> = None;
    let mut pending_window: Option<Vec<f32>> = None;

    loop {
        tokio::select! {
            result = async {
                if let Some(task) = transcribe_task.as_mut() {
                    task.await
                } else {
                    std::future::pending().await
                }
            }, if transcribe_task.is_some() => {
                transcribe_task = None;
                match result {
                    Ok(Ok(hypothesis)) if !hypothesis.is_empty() => {
                        on_model_state(ModelState::Ready);
                        let preview_text = agreement.update(&hypothesis);
                        last_stable.clone_from(&preview_text.stable);
                        publish_preview(
                            &job_id,
                            &updates,
                            &events,
                            PreviewSnapshot {
                                job_id: job_id.clone(),
                                stable: preview_text.stable,
                                provisional: preview_text.provisional,
                                speech_active: true,
                            },
                        );
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        tracing::debug!(%error, "preview transcription failed");
                        on_model_state(ModelState::Failed {
                            code: "preview_asr_error".into(),
                            message: error.to_string(),
                        });
                    }
                    Err(error) => {
                        tracing::debug!(%error, "preview transcription task failed");
                    }
                }
                if let Some(window) = pending_window.take() {
                    transcribe_task = Some(spawn_preview_transcribe(preview_asr.clone(), window));
                }
            }
            _ = step.tick() => {
                let resampled = tap.resampled_snapshot();
                let window =
                    extract_preview_window(&resampled, sample_rate, config.chunk_ms, config.overlap_ms);
                if window.len() < min_window_samples {
                    continue;
                }
                let speech_active = window_rms_energy(&window) >= config.min_rms_energy;
                if !speech_active {
                    if !last_stable.is_empty() {
                        publish_preview(
                            &job_id,
                            &updates,
                            &events,
                            PreviewSnapshot {
                                job_id: job_id.clone(),
                                stable: last_stable.clone(),
                                provisional: String::new(),
                                speech_active: false,
                            },
                        );
                    }
                    continue;
                }
                if transcribe_task.is_some() {
                    pending_window = Some(window);
                } else {
                    transcribe_task = Some(spawn_preview_transcribe(preview_asr.clone(), window));
                }
            }
        }
    }
}

fn spawn_preview_transcribe(
    preview_asr: PreviewAsrManager,
    window: Vec<f32>,
) -> JoinHandle<Result<String, AsrError>> {
    tokio::spawn(async move { preview_asr.transcribe_preview(window).await })
}

fn publish_preview(
    job_id: &JobId,
    updates: &watch::Sender<Option<PreviewSnapshot>>,
    events: &tokio::sync::broadcast::Sender<Event>,
    snapshot: PreviewSnapshot,
) {
    let _ = updates.send(Some(snapshot.clone()));
    let _ = events.send(Event::Preview {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        job_id: job_id.clone(),
        stable: snapshot.stable,
        provisional: snapshot.provisional,
        speech_active: snapshot.speech_active,
    });
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
