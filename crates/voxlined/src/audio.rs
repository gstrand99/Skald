#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    Device, SampleFormat, Stream, StreamConfig, StreamError,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rubato::{Fft, FixedSync, Resampler, audioadapter_buffers::direct::SequentialSliceOfVecs};
use thiserror::Error;
use tokio::sync::oneshot;
use voxline_core::{
    config::{AudioConfig, AudioGatesConfig, PathsConfig},
    protocol::{AudioRecording, JobId},
    runtime::runtime_dir_for,
};

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no default input device is available")]
    NoInputDevice,
    #[error("failed to query input configuration: {0}")]
    InputConfig(String),
    #[error("failed to build input stream: {0}")]
    BuildStream(String),
    #[error("failed to start input stream: {0}")]
    PlayStream(String),
    #[error("audio recorder is already active")]
    AlreadyRecording,
    #[error("there is no active recording")]
    NotRecording,
    #[error("active recording belongs to a different job")]
    WrongJob,
    #[error("audio recorder stopped unexpectedly")]
    OwnerStopped,
    #[error("audio processing failed: {0}")]
    Processing(String),
    #[error("audio input stream failed: {message}")]
    StreamFailed { message: String },
}

#[derive(Clone)]
pub struct RecordingTap {
    preview_ring: Arc<Mutex<VecDeque<f32>>>,
    sample_rate: u32,
    channels: u16,
    target_sample_rate: u32,
    max_samples: usize,
}

impl RecordingTap {
    #[must_use]
    pub fn target_sample_rate(&self) -> u32 {
        self.target_sample_rate
    }

    pub fn resampled_snapshot(&self) -> Vec<f32> {
        let raw: Vec<f32> = self
            .preview_ring
            .lock()
            .map(|ring| ring.iter().copied().collect())
            .unwrap_or_default();
        let mono = mix_to_mono(&raw, self.channels);
        let resampled = resample(&mono, self.sample_rate, self.target_sample_rate);
        voxline_core::preview::trim_to_ring_buffer(resampled, self.max_samples)
    }
}

pub struct AudioRecorder {
    commands: mpsc::Sender<OwnerCommand>,
    tap: Arc<Mutex<Option<RecordingTap>>>,
}

enum OwnerCommand {
    Start {
        job_id: JobId,
        preview_ring_buffer_seconds: Option<u64>,
        reply: oneshot::Sender<Result<(), AudioError>>,
    },
    Stop {
        job_id: JobId,
        reply: oneshot::Sender<Result<AudioRecording, AudioError>>,
    },
    Cancel {
        job_id: JobId,
        reply: oneshot::Sender<Result<(), AudioError>>,
    },
}

struct ActiveRecording {
    job_id: JobId,
    started_at: Instant,
    sample_rate: u32,
    channels: u16,
    samples: Arc<Mutex<Vec<f32>>>,
    preview_ring: Option<Arc<Mutex<VecDeque<f32>>>>,
    truncated: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
    stream: Stream,
}

impl AudioRecorder {
    pub fn spawn(config: AudioConfig, paths: PathsConfig) -> Self {
        let (commands, receiver) = mpsc::channel();
        let tap = Arc::new(Mutex::new(None));
        let tap_for_owner = tap.clone();
        thread::Builder::new()
            .name("voxline-audio-owner".into())
            .spawn(move || owner_loop(config, paths, receiver, tap_for_owner))
            .expect("audio owner thread should start");
        Self { commands, tap }
    }

    pub fn current_tap(&self) -> Option<RecordingTap> {
        self.tap.lock().ok()?.clone()
    }

    pub async fn start(
        &self,
        job_id: JobId,
        preview_ring_buffer_seconds: Option<u64>,
    ) -> Result<(), AudioError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(OwnerCommand::Start {
                job_id,
                preview_ring_buffer_seconds,
                reply,
            })
            .map_err(|_| AudioError::OwnerStopped)?;
        response.await.map_err(|_| AudioError::OwnerStopped)?
    }

    pub async fn stop(&self, job_id: JobId) -> Result<AudioRecording, AudioError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(OwnerCommand::Stop { job_id, reply })
            .map_err(|_| AudioError::OwnerStopped)?;
        response.await.map_err(|_| AudioError::OwnerStopped)?
    }

    pub async fn cancel(&self, job_id: JobId) -> Result<(), AudioError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(OwnerCommand::Cancel { job_id, reply })
            .map_err(|_| AudioError::OwnerStopped)?;
        response.await.map_err(|_| AudioError::OwnerStopped)?
    }
}

#[allow(clippy::needless_pass_by_value)]
fn owner_loop(
    config: AudioConfig,
    paths: PathsConfig,
    receiver: mpsc::Receiver<OwnerCommand>,
    tap_slot: Arc<Mutex<Option<RecordingTap>>>,
) {
    let mut active: Option<ActiveRecording> = None;
    let mut stream_failure: Option<(JobId, String)> = None;

    loop {
        let command = if active.is_some() {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => {
                    handle_active_stream_error(&mut active, &tap_slot, &mut stream_failure);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            receiver.recv().ok()
        };

        let Some(command) = command else {
            break;
        };

        match command {
            OwnerCommand::Start {
                job_id,
                preview_ring_buffer_seconds,
                reply,
            } => {
                let result = if active.is_some() {
                    Err(AudioError::AlreadyRecording)
                } else {
                    start_recording(job_id, &config, preview_ring_buffer_seconds).map(|recording| {
                        if let Some(seconds) = preview_ring_buffer_seconds {
                            let preview_ring = recording
                                .preview_ring
                                .clone()
                                .expect("preview ring exists when preview is enabled");
                            let max_samples = usize::try_from(
                                seconds.saturating_mul(u64::from(config.target_sample_rate)),
                            )
                            .unwrap_or(usize::MAX);
                            if let Ok(mut slot) = tap_slot.lock() {
                                *slot = Some(RecordingTap {
                                    preview_ring,
                                    sample_rate: recording.sample_rate,
                                    channels: recording.channels,
                                    target_sample_rate: config.target_sample_rate,
                                    max_samples,
                                });
                            }
                        }
                        active = Some(recording);
                    })
                };
                let _ = reply.send(result);
            }
            OwnerCommand::Stop { job_id, reply } => {
                let result = if let Some(error) =
                    take_pending_stream_failure(&mut stream_failure, &job_id)
                {
                    Err(error)
                } else {
                    take_matching(&mut active, &job_id).and_then(|recording| {
                        if let Ok(mut slot) = tap_slot.lock() {
                            *slot = None;
                        }
                        finish_recording(recording, &config, &paths)
                    })
                };
                let _ = reply.send(result);
            }
            OwnerCommand::Cancel { job_id, reply } => {
                let result = if let Some(error) =
                    take_pending_stream_failure(&mut stream_failure, &job_id)
                {
                    Err(error)
                } else {
                    take_matching(&mut active, &job_id).map(drop)
                };
                if let Ok(mut slot) = tap_slot.lock() {
                    *slot = None;
                }
                let _ = reply.send(result);
            }
        }
    }
}

fn take_pending_stream_failure(
    pending: &mut Option<(JobId, String)>,
    job_id: &JobId,
) -> Option<AudioError> {
    if pending.as_ref().is_some_and(|(id, _)| id == job_id) {
        let (_, message) = pending.take().unwrap();
        Some(AudioError::StreamFailed { message })
    } else {
        None
    }
}

fn handle_active_stream_error(
    active: &mut Option<ActiveRecording>,
    tap_slot: &Arc<Mutex<Option<RecordingTap>>>,
    pending: &mut Option<(JobId, String)>,
) {
    let Some(recording) = active.as_ref() else {
        return;
    };
    let message = recording
        .stream_error
        .lock()
        .ok()
        .and_then(|slot| slot.clone());
    let Some(message) = message else {
        return;
    };
    let job_id = recording.job_id.clone();
    active.take();
    if let Ok(mut slot) = tap_slot.lock() {
        *slot = None;
    }
    *pending = Some((job_id, message));
}

fn take_matching(
    active: &mut Option<ActiveRecording>,
    job_id: &JobId,
) -> Result<ActiveRecording, AudioError> {
    let recording = active.as_ref().ok_or(AudioError::NotRecording)?;
    if recording.job_id != *job_id {
        return Err(AudioError::WrongJob);
    }
    active.take().ok_or(AudioError::NotRecording)
}

fn max_capture_samples(max_record_seconds: u64, sample_rate: u32, channels: u16) -> usize {
    usize::try_from(
        max_record_seconds
            .saturating_mul(u64::from(sample_rate))
            .saturating_mul(u64::from(channels)),
    )
    .unwrap_or(usize::MAX)
}

fn preview_ring_capacity(preview_seconds: u64, sample_rate: u32, channels: u16) -> usize {
    max_capture_samples(preview_seconds, sample_rate, channels)
}

fn start_recording(
    job_id: JobId,
    config: &AudioConfig,
    preview_ring_buffer_seconds: Option<u64>,
) -> Result<ActiveRecording, AudioError> {
    let host = cpal::default_host();
    let device = if config.device == "default" {
        host.default_input_device()
            .ok_or(AudioError::NoInputDevice)?
    } else {
        host.input_devices()
            .map_err(|error| AudioError::InputConfig(error.to_string()))?
            .find(|device| device.name().is_ok_and(|name| name == config.device))
            .ok_or(AudioError::NoInputDevice)?
    };
    let supported = device
        .default_input_config()
        .map_err(|error| AudioError::InputConfig(error.to_string()))?;
    let sample_format = supported.sample_format();
    let stream_config: StreamConfig = supported.into();
    let sample_rate = stream_config.sample_rate.0;
    let channels = stream_config.channels;
    let max_samples_cap = max_capture_samples(config.max_record_seconds, sample_rate, channels);
    let preview_ring = preview_ring_buffer_seconds.map(|seconds| {
        Arc::new(Mutex::new(VecDeque::with_capacity(preview_ring_capacity(
            seconds,
            sample_rate,
            channels,
        ))))
    });
    let preview_ring_cap = preview_ring_buffer_seconds.map_or(0, |seconds| {
        preview_ring_capacity(seconds, sample_rate, channels)
    });
    let samples = Arc::new(Mutex::new(Vec::new()));
    let truncated = Arc::new(AtomicBool::new(false));
    let stream_error = Arc::new(Mutex::new(None));
    let capture_context = CaptureContext {
        samples: &samples,
        max_samples_cap,
        truncated: &truncated,
        preview_ring: preview_ring.as_ref(),
        preview_ring_cap,
    };
    let stream = build_stream(
        &device,
        &stream_config,
        sample_format,
        &capture_context,
        stream_error.clone(),
    )?;
    stream
        .play()
        .map_err(|error| AudioError::PlayStream(error.to_string()))?;
    Ok(ActiveRecording {
        job_id,
        started_at: Instant::now(),
        sample_rate,
        channels,
        samples,
        preview_ring,
        truncated,
        stream_error,
        stream,
    })
}

struct CaptureContext<'a> {
    samples: &'a Arc<Mutex<Vec<f32>>>,
    max_samples_cap: usize,
    truncated: &'a Arc<AtomicBool>,
    preview_ring: Option<&'a Arc<Mutex<VecDeque<f32>>>>,
    preview_ring_cap: usize,
}

fn append_capture_samples(context: &CaptureContext<'_>, converted: &[f32]) {
    if let Ok(mut output) = context.samples.lock() {
        for &sample in converted {
            if output.len() < context.max_samples_cap {
                output.push(sample);
            } else if !context.truncated.swap(true, Ordering::Relaxed) {
                tracing::warn!("recording reached configured maximum length");
            }
        }
    }
    if let Some(ring) = context.preview_ring
        && let Ok(mut ring) = ring.lock()
    {
        for &sample in converted {
            ring.push_back(sample);
            while ring.len() > context.preview_ring_cap {
                ring.pop_front();
            }
        }
    }
}

fn build_stream(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    context: &CaptureContext<'_>,
    stream_error: Arc<Mutex<Option<String>>>,
) -> Result<Stream, AudioError> {
    let error_callback = move |error: StreamError| {
        tracing::warn!(%error, "audio input stream error");
        if let Ok(mut slot) = stream_error.lock()
            && slot.is_none()
        {
            *slot = Some(error.to_string());
        }
    };
    macro_rules! stream {
        ($sample:ty, $convert:expr) => {{
            let samples = context.samples.clone();
            let max_samples_cap = context.max_samples_cap;
            let truncated = context.truncated.clone();
            let preview_ring = context.preview_ring.cloned();
            let preview_ring_cap = context.preview_ring_cap;
            device.build_input_stream(
                config,
                move |data: &[$sample], _| {
                    let capture = CaptureContext {
                        samples: &samples,
                        max_samples_cap,
                        truncated: &truncated,
                        preview_ring: preview_ring.as_ref(),
                        preview_ring_cap,
                    };
                    let converted: Vec<f32> = data.iter().copied().map($convert).collect();
                    append_capture_samples(&capture, &converted);
                },
                error_callback,
                None,
            )
        }};
    }
    let result = match format {
        SampleFormat::F32 => stream!(f32, |sample: f32| sample),
        SampleFormat::F64 => stream!(f64, |sample: f64| sample as f32),
        SampleFormat::I8 => stream!(i8, |sample: i8| f32::from(sample) / f32::from(i8::MAX)),
        SampleFormat::I16 => stream!(i16, |sample: i16| f32::from(sample) / f32::from(i16::MAX)),
        SampleFormat::I32 => stream!(i32, |sample: i32| sample as f32 / i32::MAX as f32),
        SampleFormat::I64 => stream!(i64, |sample: i64| sample as f32 / i64::MAX as f32),
        SampleFormat::U8 => stream!(u8, |sample: u8| (f32::from(sample) / f32::from(u8::MAX))
            * 2.0
            - 1.0),
        SampleFormat::U16 => stream!(u16, |sample: u16| (f32::from(sample) / f32::from(u16::MAX))
            * 2.0
            - 1.0),
        SampleFormat::U32 => stream!(u32, |sample: u32| (sample as f32 / u32::MAX as f32) * 2.0
            - 1.0),
        SampleFormat::U64 => stream!(u64, |sample: u64| (sample as f32 / u64::MAX as f32) * 2.0
            - 1.0),
        other => {
            return Err(AudioError::InputConfig(format!(
                "unsupported sample format {other}"
            )));
        }
    };
    result.map_err(|error| AudioError::BuildStream(error.to_string()))
}

fn finish_recording(
    recording: ActiveRecording,
    config: &AudioConfig,
    paths: &PathsConfig,
) -> Result<AudioRecording, AudioError> {
    let ActiveRecording {
        job_id,
        started_at,
        sample_rate,
        channels,
        samples,
        truncated,
        stream,
        ..
    } = recording;
    // Dropping the stream stops the CPAL callback; the sample mutex serializes any in-flight
    // append, so no extra drain delay is required before reading the buffer.
    drop(stream);
    let samples = samples
        .lock()
        .map_err(|_| AudioError::Processing("audio sample buffer was poisoned".into()))?
        .clone();
    let mono = mix_to_mono(&samples, channels);
    let resampled = resample(&mono, sample_rate, config.target_sample_rate);
    let (rms_energy, peak_energy) = energy(&resampled);
    let duration_ms = samples_to_duration_ms(resampled.len(), config.target_sample_rate);
    let speech_detected = duration_ms >= config.gates.min_record_ms
        && rms_energy >= config.gates.min_rms_energy
        && peak_energy >= config.gates.min_peak_energy;
    let wav_path = runtime_dir_for(paths)
        .map_err(|error| AudioError::Processing(error.to_string()))?
        .join(format!("{}.wav", job_id.0));
    write_wav(&wav_path, &resampled, config.target_sample_rate)?;
    let elapsed_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(AudioRecording {
        job_id,
        wav_path,
        duration_ms: duration_ms.min(elapsed_ms.saturating_add(100)),
        sample_rate: config.target_sample_rate,
        channels: 1,
        rms_energy,
        peak_energy,
        speech_detected,
        truncated: truncated.load(Ordering::Relaxed),
    })
}

fn mix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let channel_count = usize::from(channels.max(1));
    samples
        .chunks_exact(channel_count)
        .map(|frame| frame.iter().sum::<f32>() / channel_count as f32)
        .collect()
}

fn resample(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if samples.is_empty() || source_rate == target_rate {
        return samples.to_vec();
    }
    let source_rate = usize::try_from(source_rate).unwrap_or(usize::MAX);
    let target_rate = usize::try_from(target_rate).unwrap_or(usize::MAX);
    let mut resampler = Fft::<f32>::new(source_rate, target_rate, 1024, 1, 1, FixedSync::Both)
        .expect("supported sample rates should construct an FFT resampler");
    let input_data = vec![samples.to_vec()];
    let input =
        SequentialSliceOfVecs::new(&input_data, 1, samples.len()).expect("mono input adapter");
    let output_len = resampler.process_all_needed_output_len(samples.len());
    let mut output_data = vec![vec![0.0f32; output_len]];
    let mut output_adapter = SequentialSliceOfVecs::new_mut(&mut output_data, 1, output_len)
        .expect("mono output adapter");
    let (_, written) = resampler
        .process_all_into_buffer(&input, &mut output_adapter, samples.len(), None)
        .expect("FFT resampling should succeed for in-memory mono audio");
    output_data[0].truncate(written);
    output_data[0].clone()
}

fn energy(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let sum_squares = samples.iter().map(|sample| sample * sample).sum::<f32>();
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0, f32::max);
    ((sum_squares / samples.len() as f32).sqrt(), peak)
}

fn samples_to_duration_ms(samples: usize, sample_rate: u32) -> u64 {
    u64::try_from(samples)
        .unwrap_or(u64::MAX)
        .saturating_mul(1_000)
        / u64::from(sample_rate)
}

pub fn recording_from_existing_wav(
    path: &Path,
    gates: &AudioGatesConfig,
    target_sample_rate: u32,
) -> Result<AudioRecording, AudioError> {
    let mut reader =
        hound::WavReader::open(path).map_err(|error| AudioError::Processing(error.to_string()))?;
    let spec = reader.spec();
    let channels = spec.channels;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|sample| sample.map_err(|error| AudioError::Processing(error.to_string())))
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let divisor = f32::from(1_u16 << spec.bits_per_sample.saturating_sub(1).min(15));
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample
                        .map(|value| value as f32 / divisor)
                        .map_err(|error| AudioError::Processing(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    let mono = mix_to_mono(&samples, channels);
    let resampled = resample(&mono, spec.sample_rate, target_sample_rate);
    let (rms_energy, peak_energy) = energy(&resampled);
    let duration_ms = samples_to_duration_ms(resampled.len(), target_sample_rate);
    let speech_detected = duration_ms >= gates.min_record_ms
        && rms_energy >= gates.min_rms_energy
        && peak_energy >= gates.min_peak_energy;
    Ok(AudioRecording {
        job_id: JobId::new(),
        wav_path: path.to_path_buf(),
        duration_ms,
        sample_rate: target_sample_rate,
        channels: 1,
        rms_energy,
        peak_energy,
        speech_detected,
        truncated: false,
    })
}

fn write_wav(path: &PathBuf, samples: &[f32], sample_rate: u32) -> Result<(), AudioError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| AudioError::Processing(error.to_string()))?;
    }
    let specification = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, specification)
        .map_err(|error| AudioError::Processing(error.to_string()))?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        writer
            .write_sample(value)
            .map_err(|error| AudioError::Processing(error.to_string()))?;
    }
    writer
        .finalize()
        .map_err(|error| AudioError::Processing(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_stereo_frames_to_mono() {
        assert_eq!(mix_to_mono(&[1.0, -1.0, 0.5, 0.5], 2), vec![0.0, 0.5]);
    }

    #[test]
    fn resamples_to_requested_rate() {
        let sample_rate = 48_000;
        let target_rate = 16_000;
        let input: Vec<f32> = (0..sample_rate)
            .map(|index| {
                (std::f32::consts::TAU * 1_000.0 * index as f32 / sample_rate as f32).sin()
            })
            .collect();
        let output = resample(&input, sample_rate, target_rate);
        let expected_len = sample_rate as usize * target_rate as usize / sample_rate as usize;
        let tolerance = (expected_len / 100).max(1);
        assert!(
            output.len().abs_diff(expected_len) <= tolerance,
            "expected length near {expected_len}, got {}",
            output.len()
        );
        let peak = output
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0f32, f32::max);
        assert!(
            peak > 0.1,
            "resampled sine should retain energy, peak={peak}"
        );
    }

    #[test]
    fn calculates_energy() {
        let (rms, peak) = energy(&[0.5, -0.5]);
        assert!((rms - 0.5).abs() < f32::EPSILON);
        assert!((peak - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn take_pending_stream_failure_returns_error_for_matching_job() {
        let job_id = JobId::new();
        let mut pending = Some((job_id.clone(), "device unplugged".into()));
        let error = take_pending_stream_failure(&mut pending, &job_id)
            .expect("matching job should return stream failure");
        assert!(matches!(error, AudioError::StreamFailed { .. }));
        assert!(pending.is_none());
    }

    #[test]
    fn take_pending_stream_failure_ignores_other_jobs() {
        let active_job = JobId::new();
        let other_job = JobId::new();
        let mut pending = Some((active_job, "device unplugged".into()));
        assert!(take_pending_stream_failure(&mut pending, &other_job).is_none());
        assert!(pending.is_some());
    }
}
