use std::sync::mpsc;

use skald_core::config::{AsrConfig, PreviewConfig};
use tokio::sync::oneshot;

use crate::asr::{AsrError, WhisperEngine};

#[derive(Clone)]
pub struct PreviewAsrManager {
    commands: mpsc::Sender<PreviewCommand>,
}

enum PreviewCommand {
    Transcribe {
        samples: Vec<f32>,
        reply: oneshot::Sender<Result<String, AsrError>>,
    },
    Unload {
        reply: oneshot::Sender<()>,
    },
}

impl PreviewAsrManager {
    pub fn spawn(preview: &PreviewConfig, asr: &AsrConfig) -> Self {
        let config = preview.to_asr_config(asr);
        let (commands, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("skald-preview-asr".into())
            .spawn(move || {
                let mut engine = WhisperEngine::new(config);
                if let Err(error) = engine.load() {
                    tracing::warn!(%error, "preview ASR warm load failed");
                }
                preview_worker_loop(&mut engine, receiver);
            })
            .expect("preview ASR worker should start");
        Self { commands }
    }

    pub async fn transcribe_preview(&self, samples: Vec<f32>) -> Result<String, AsrError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(PreviewCommand::Transcribe { samples, reply })
            .map_err(|_| AsrError::WorkerStopped)?;
        response.await.map_err(|_| AsrError::WorkerStopped)?
    }

    pub async fn unload(&self) -> Result<(), AsrError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(PreviewCommand::Unload { reply })
            .map_err(|_| AsrError::WorkerStopped)?;
        response.await.map_err(|_| AsrError::WorkerStopped)?;
        Ok(())
    }
}

#[allow(clippy::needless_pass_by_value)]
fn preview_worker_loop(engine: &mut WhisperEngine, receiver: mpsc::Receiver<PreviewCommand>) {
    loop {
        let timeout = engine.idle_timeout();
        let command = match timeout {
            Some(timeout) => match receiver.recv_timeout(timeout) {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    engine.unload();
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
            None => match receiver.recv() {
                Ok(command) => command,
                Err(_) => return,
            },
        };
        match command {
            PreviewCommand::Transcribe { samples, reply } => {
                let _ = reply.send(engine.transcribe_preview_samples(&samples));
            }
            PreviewCommand::Unload { reply } => {
                engine.unload();
                let _ = reply.send(());
            }
        }
    }
}
