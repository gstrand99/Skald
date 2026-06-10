use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use thiserror::Error;
use tokio::sync::oneshot;
use voxline_core::{
    config::{AsrConfig, VocabularyConfig},
    protocol::{AsrBenchmark, Transcript, TranscriptSegment},
};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
    convert_integer_to_float_audio,
};

#[derive(Debug, Error)]
pub enum AsrError {
    #[error("model not found: {path}")]
    ModelNotFound { path: PathBuf },
    #[error("model load failed: {message}")]
    ModelLoadFailed { message: String },
    #[error("transcription failed: {message}")]
    TranscriptionFailed { message: String },
    #[error("unsupported backend feature: {feature}")]
    UnsupportedFeature { feature: String },
    #[error("ASR worker stopped unexpectedly")]
    WorkerStopped,
}

pub struct AsrManager {
    commands: mpsc::Sender<Command>,
}

enum Command {
    Load(oneshot::Sender<Result<u64, AsrError>>),
    Unload(oneshot::Sender<Result<(), AsrError>>),
    Transcribe {
        path: PathBuf,
        reply: oneshot::Sender<Result<(Transcript, AsrBenchmark), AsrError>>,
    },
}

struct Worker {
    config: AsrConfig,
    vocabulary: VocabularyConfig,
    context: Option<WhisperContext>,
    last_used: Option<Instant>,
}

impl AsrManager {
    pub fn spawn(config: AsrConfig, vocabulary: VocabularyConfig) -> Self {
        let warm = config.lifecycle.mode == "keep_warm" && config.lifecycle.warm_on_daemon_start;
        let (commands, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("voxline-asr-worker".into())
            .spawn(move || {
                let mut worker = Worker {
                    config,
                    vocabulary,
                    context: None,
                    last_used: None,
                };
                if warm && let Err(error) = worker.load() {
                    tracing::warn!(%error, "ASR warm load failed");
                }
                worker.run(receiver);
            })
            .expect("ASR worker should start");
        Self { commands }
    }

    pub async fn load(&self) -> Result<u64, AsrError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Load(reply))
            .map_err(|_| AsrError::WorkerStopped)?;
        response.await.map_err(|_| AsrError::WorkerStopped)?
    }

    pub async fn unload(&self) -> Result<(), AsrError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Unload(reply))
            .map_err(|_| AsrError::WorkerStopped)?;
        response.await.map_err(|_| AsrError::WorkerStopped)?
    }

    pub async fn transcribe(&self, path: PathBuf) -> Result<(Transcript, AsrBenchmark), AsrError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Transcribe { path, reply })
            .map_err(|_| AsrError::WorkerStopped)?;
        response.await.map_err(|_| AsrError::WorkerStopped)?
    }
}

impl Worker {
    #[allow(clippy::needless_pass_by_value)]
    fn run(&mut self, receiver: mpsc::Receiver<Command>) {
        loop {
            let timeout = self.idle_timeout();
            let command = match timeout {
                Some(timeout) => match receiver.recv_timeout(timeout) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        self.context = None;
                        self.last_used = None;
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
                Command::Load(reply) => {
                    let _ = reply.send(self.load());
                }
                Command::Unload(reply) => {
                    self.context = None;
                    self.last_used = None;
                    let _ = reply.send(Ok(()));
                }
                Command::Transcribe { path, reply } => {
                    let _ = reply.send(self.transcribe(&path));
                }
            }
        }
    }

    fn idle_timeout(&self) -> Option<Duration> {
        if self.config.lifecycle.mode != "keep_warm"
            || self.context.is_none()
            || self.config.lifecycle.idle_unload_seconds == 0
        {
            return None;
        }
        let idle = self.last_used?.elapsed();
        Some(Duration::from_secs(self.config.lifecycle.idle_unload_seconds).saturating_sub(idle))
    }

    fn load(&mut self) -> Result<u64, AsrError> {
        if self.context.is_some() {
            return Ok(0);
        }
        let path = expand_home(&self.config.model_path);
        if !path.is_file() {
            return Err(AsrError::ModelNotFound { path });
        }
        if self.config.gpu && !cfg!(feature = "asr-whisper-rs-cuda") {
            return Err(AsrError::UnsupportedFeature {
                feature: "CUDA support was not enabled at build time".into(),
            });
        }
        let started = Instant::now();
        let mut parameters = WhisperContextParameters::default();
        parameters.use_gpu(self.config.gpu);
        let context = WhisperContext::new_with_params(
            path.to_str().ok_or_else(|| AsrError::ModelLoadFailed {
                message: "model path is not valid UTF-8".into(),
            })?,
            parameters,
        )
        .map_err(|error| AsrError::ModelLoadFailed {
            message: error.to_string(),
        })?;
        self.context = Some(context);
        self.last_used = Some(Instant::now());
        Ok(elapsed_ms(started))
    }

    fn transcribe(&mut self, path: &Path) -> Result<(Transcript, AsrBenchmark), AsrError> {
        let model_load_ms = self.load()?;
        let (audio, duration_ms) = read_wav(path)?;
        let mut state = self
            .context
            .as_ref()
            .expect("model loaded")
            .create_state()
            .map_err(|error| AsrError::TranscriptionFailed {
                message: error.to_string(),
            })?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(i32::from(self.config.threads));
        params.set_language(Some(&self.config.language));
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        let prompt = self.initial_prompt();
        if let Some(prompt) = prompt.as_deref() {
            params.set_initial_prompt(prompt);
        }
        let started = Instant::now();
        state
            .full(params, &audio)
            .map_err(|error| AsrError::TranscriptionFailed {
                message: error.to_string(),
            })?;
        let transcribe_ms = elapsed_ms(started);
        self.last_used = Some(Instant::now());
        let mut segments = Vec::new();
        for segment in state.as_iter() {
            let text = segment
                .to_str_lossy()
                .map_err(|error| AsrError::TranscriptionFailed {
                    message: error.to_string(),
                })?
                .trim()
                .to_owned();
            if !text.is_empty() {
                segments.push(TranscriptSegment {
                    start_ms: centiseconds_to_ms(segment.start_timestamp()),
                    end_ms: centiseconds_to_ms(segment.end_timestamp()),
                    text,
                });
            }
        }
        let raw = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let text = filter_hallucination(
            &apply_replacements(raw.trim(), &self.vocabulary),
            &self.config,
        );
        let transcript = Transcript {
            text,
            language: Some(self.config.language.clone()),
            duration_ms: Some(duration_ms),
            segments,
        };
        if self.config.lifecycle.mode == "on_demand" {
            self.context = None;
        }
        Ok((
            transcript,
            AsrBenchmark {
                model_load_ms,
                transcribe_ms,
                audio_duration_ms: duration_ms,
            },
        ))
    }

    fn initial_prompt(&self) -> Option<String> {
        if !self.vocabulary.enabled || !self.vocabulary.initial_prompt_enabled {
            return None;
        }
        let values = self
            .vocabulary
            .phrases
            .iter()
            .map(|phrase| phrase.text.trim())
            .filter(|phrase| !phrase.is_empty())
            .collect::<Vec<_>>();
        (!values.is_empty()).then(|| values.join(", "))
    }
}

fn read_wav(path: &Path) -> Result<(Vec<f32>, u64), AsrError> {
    let mut reader =
        hound::WavReader::open(path).map_err(|error| AsrError::TranscriptionFailed {
            message: error.to_string(),
        })?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(AsrError::TranscriptionFailed {
            message: "audio must be 16 kHz mono 16-bit PCM WAV".into(),
        });
    }
    let samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AsrError::TranscriptionFailed {
            message: error.to_string(),
        })?;
    let mut audio = vec![0.0; samples.len()];
    convert_integer_to_float_audio(&samples, &mut audio).map_err(|error| {
        AsrError::TranscriptionFailed {
            message: error.to_string(),
        }
    })?;
    let duration_ms = u64::try_from(samples.len()).unwrap_or(u64::MAX) * 1_000 / 16_000;
    Ok((audio, duration_ms))
}

fn apply_replacements(input: &str, vocabulary: &VocabularyConfig) -> String {
    if !vocabulary.enabled || !vocabulary.post_replace_enabled {
        return input.to_owned();
    }
    vocabulary
        .replacements
        .iter()
        .fold(input.to_owned(), |text, rule| {
            replace_whole_words(&text, &rule.from, &rule.to, rule.case_sensitive)
        })
}

fn replace_whole_words(input: &str, from: &str, to: &str, case_sensitive: bool) -> String {
    if from.is_empty() {
        return input.to_owned();
    }
    let haystack = if case_sensitive {
        input.to_owned()
    } else {
        input.to_ascii_lowercase()
    };
    let needle = if case_sensitive {
        from.to_owned()
    } else {
        from.to_ascii_lowercase()
    };
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative) = haystack[cursor..].find(&needle) {
        let start = cursor + relative;
        let end = start + needle.len();
        let left_boundary = start == 0
            || !input[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
        let right_boundary = end == input.len()
            || !input[end..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric);
        if left_boundary && right_boundary {
            output.push_str(&input[cursor..start]);
            output.push_str(to);
            cursor = end;
        } else {
            let next = input[start..]
                .chars()
                .next()
                .map_or(end, |character| start + character.len_utf8());
            output.push_str(&input[cursor..next]);
            cursor = next;
        }
    }
    output.push_str(&input[cursor..]);
    output
}

fn filter_hallucination(input: &str, config: &AsrConfig) -> String {
    if !config.hallucination_filter.enabled || input.split_whitespace().count() > 5 {
        return input.to_owned();
    }
    let normalized = input.trim().to_ascii_lowercase();
    if config
        .hallucination_filter
        .phrases
        .iter()
        .any(|phrase| normalized == phrase.trim().to_ascii_lowercase())
    {
        String::new()
    } else {
        input.to_owned()
    }
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(relative) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(relative);
    }
    PathBuf::from(path)
}

fn centiseconds_to_ms(value: i64) -> u64 {
    u64::try_from(value.max(0))
        .unwrap_or(u64::MAX)
        .saturating_mul(10)
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_complete_phrases() {
        let config = VocabularyConfig::default();
        assert_eq!(
            apply_replacements("open router works", &config),
            "OpenRouter works"
        );
        assert_eq!(
            apply_replacements("reopen router", &config),
            "reopen router"
        );
    }

    #[test]
    fn filters_only_short_exact_hallucinations() {
        let config = AsrConfig::default();
        assert_eq!(filter_hallucination("Thank you.", &config), "");
        assert_eq!(
            filter_hallucination("Thank you. This is legitimate longer text", &config),
            "Thank you. This is legitimate longer text"
        );
    }
}
