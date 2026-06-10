#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::{
    fs,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use cpal::{
    Device, SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use thiserror::Error;
use tokio::sync::oneshot;
use voxline_core::{
    config::AudioConfig,
    protocol::{AudioRecording, JobId},
    runtime::runtime_dir,
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
}

pub struct AudioRecorder {
    commands: mpsc::Sender<OwnerCommand>,
}

enum OwnerCommand {
    Start {
        job_id: JobId,
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
    samples: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
    stream: Stream,
}

impl AudioRecorder {
    pub fn spawn(config: AudioConfig) -> Self {
        let (commands, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("voxline-audio-owner".into())
            .spawn(move || owner_loop(config, receiver))
            .expect("audio owner thread should start");
        Self { commands }
    }

    pub async fn start(&self, job_id: JobId) -> Result<(), AudioError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(OwnerCommand::Start { job_id, reply })
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
fn owner_loop(config: AudioConfig, receiver: mpsc::Receiver<OwnerCommand>) {
    let mut active: Option<ActiveRecording> = None;
    while let Ok(command) = receiver.recv() {
        match command {
            OwnerCommand::Start { job_id, reply } => {
                let result = if active.is_some() {
                    Err(AudioError::AlreadyRecording)
                } else {
                    start_recording(job_id, &config).map(|recording| active = Some(recording))
                };
                let _ = reply.send(result);
            }
            OwnerCommand::Stop { job_id, reply } => {
                let result = take_matching(&mut active, &job_id)
                    .and_then(|recording| finish_recording(recording, &config));
                let _ = reply.send(result);
            }
            OwnerCommand::Cancel { job_id, reply } => {
                let result = take_matching(&mut active, &job_id).map(drop);
                let _ = reply.send(result);
            }
        }
    }
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

fn start_recording(job_id: JobId, config: &AudioConfig) -> Result<ActiveRecording, AudioError> {
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
    let samples = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let stream = build_stream(&device, &stream_config, sample_format, &samples)?;
    stream
        .play()
        .map_err(|error| AudioError::PlayStream(error.to_string()))?;
    Ok(ActiveRecording {
        job_id,
        started_at: Instant::now(),
        sample_rate: stream_config.sample_rate.0,
        channels: stream_config.channels,
        samples,
        stream,
    })
}

fn build_stream(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    samples: &std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
) -> Result<Stream, AudioError> {
    let error_callback = |error| tracing::warn!(%error, "audio input stream error");
    macro_rules! stream {
        ($sample:ty, $convert:expr) => {{
            let samples = samples.clone();
            device.build_input_stream(
                config,
                move |data: &[$sample], _| {
                    if let Ok(mut output) = samples.lock() {
                        output.extend(data.iter().copied().map($convert));
                    }
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
) -> Result<AudioRecording, AudioError> {
    let ActiveRecording {
        job_id,
        started_at,
        sample_rate,
        channels,
        samples,
        stream,
    } = recording;
    drop(stream);
    thread::sleep(Duration::from_millis(20));
    let samples = std::sync::Arc::try_unwrap(samples)
        .map_err(|_| AudioError::Processing("audio callback still owns samples".into()))?
        .into_inner()
        .map_err(|_| AudioError::Processing("audio sample buffer was poisoned".into()))?;
    let mono = mix_to_mono(&samples, channels);
    let resampled = resample_linear(&mono, sample_rate, config.target_sample_rate);
    let (rms_energy, peak_energy) = energy(&resampled);
    let duration_ms = samples_to_duration_ms(resampled.len(), config.target_sample_rate);
    let speech_detected = duration_ms >= config.gates.min_record_ms
        && rms_energy >= config.gates.min_rms_energy
        && peak_energy >= config.gates.min_peak_energy;
    let wav_path = runtime_dir()
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
    })
}

fn mix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let channel_count = usize::from(channels.max(1));
    samples
        .chunks_exact(channel_count)
        .map(|frame| frame.iter().sum::<f32>() / channel_count as f32)
        .collect()
}

fn resample_linear(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if samples.is_empty() || source_rate == target_rate {
        return samples.to_vec();
    }
    let output_len = samples.len().saturating_mul(target_rate as usize) / source_rate as usize;
    (0..output_len)
        .map(|index| {
            let source_position = index as f64 * f64::from(source_rate) / f64::from(target_rate);
            let lower = source_position.floor() as usize;
            let upper = (lower + 1).min(samples.len() - 1);
            let fraction = (source_position - lower as f64) as f32;
            samples[lower] + (samples[upper] - samples[lower]) * fraction
        })
        .collect()
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
        let input = vec![0.0; 48_000];
        assert_eq!(resample_linear(&input, 48_000, 16_000).len(), 16_000);
    }

    #[test]
    fn calculates_energy() {
        let (rms, peak) = energy(&[0.5, -0.5]);
        assert!((rms - 0.5).abs() < f32::EPSILON);
        assert!((peak - 0.5).abs() < f32::EPSILON);
    }
}
