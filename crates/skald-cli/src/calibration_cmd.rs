use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use cpal::{
    Device, SampleFormat, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use serde::Serialize;
use skald_core::config::{AudioGatesConfig, Config};

use crate::CalibrateCommands;

#[derive(Debug, Serialize)]
struct CalibrationReport {
    seconds: u64,
    device: String,
    sample_rate: u32,
    channels: u16,
    measured: MeasuredLevels,
    current: GateLevels,
    recommended: GateLevels,
    applied: bool,
    config_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct MeasuredLevels {
    duration_ms: u64,
    rms_energy: f32,
    peak_energy: f32,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_field_names)]
struct GateLevels {
    min_record_ms: u64,
    min_rms_energy: f32,
    min_peak_energy: f32,
}

pub(crate) fn run(command: &CalibrateCommands) -> Result<()> {
    match command {
        CalibrateCommands::Mic {
            seconds,
            apply,
            json,
        } => calibrate_mic(*seconds, *apply, *json),
    }
}

fn calibrate_mic(seconds: u64, apply: bool, json: bool) -> Result<()> {
    if seconds == 0 {
        bail!("--seconds must be greater than 0");
    }
    let mut config = Config::load_or_default()?;
    let sample = capture_ambient_sample(&config, Duration::from_secs(seconds))?;
    let current = GateLevels::from(&config.audio.gates);
    let recommended = recommend_gates(&config.audio.gates, sample.rms_energy, sample.peak_energy);
    let path = if apply {
        config.audio.gates.min_record_ms = recommended.min_record_ms;
        config.audio.gates.min_rms_energy = recommended.min_rms_energy;
        config.audio.gates.min_peak_energy = recommended.min_peak_energy;
        Some(config.save()?.display().to_string())
    } else {
        None
    };
    let report = CalibrationReport {
        seconds,
        device: sample.device,
        sample_rate: sample.sample_rate,
        channels: sample.channels,
        measured: MeasuredLevels {
            duration_ms: sample.duration_ms,
            rms_energy: sample.rms_energy,
            peak_energy: sample.peak_energy,
        },
        current,
        recommended,
        applied: apply,
        config_path: path,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    Ok(())
}

struct AmbientSample {
    device: String,
    sample_rate: u32,
    channels: u16,
    duration_ms: u64,
    rms_energy: f32,
    peak_energy: f32,
}

fn capture_ambient_sample(config: &Config, duration: Duration) -> Result<AmbientSample> {
    let host = cpal::default_host();
    let device = input_device(&host, &config.audio.device)?;
    let device_name = device.name().unwrap_or_else(|_| "unknown".into());
    let supported = device
        .default_input_config()
        .context("failed to query input configuration")?;
    let sample_format = supported.sample_format();
    let stream_config: StreamConfig = supported.into();
    let sample_rate = stream_config.sample_rate.0;
    let channels = stream_config.channels;
    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let stream = build_stream(&device, &stream_config, sample_format, &samples)?;
    stream.play().context("failed to start input stream")?;
    thread::sleep(duration);
    drop(stream);
    let samples = samples
        .lock()
        .map_err(|_| anyhow::anyhow!("audio sample buffer was poisoned"))?
        .clone();
    if samples.is_empty() {
        bail!("microphone produced no samples");
    }
    let (rms_energy, peak_energy) = energy(&samples);
    let duration_ms = u64::try_from(samples.len())
        .unwrap_or(u64::MAX)
        .saturating_mul(1_000)
        / u64::from(sample_rate)
        / u64::from(channels.max(1));
    Ok(AmbientSample {
        device: device_name,
        sample_rate,
        channels,
        duration_ms,
        rms_energy,
        peak_energy,
    })
}

fn input_device(host: &cpal::Host, configured: &str) -> Result<Device> {
    if configured == "default" {
        return host
            .default_input_device()
            .context("no default input device is available");
    }
    host.input_devices()
        .context("failed to list input devices")?
        .find(|device| device.name().is_ok_and(|name| name == configured))
        .with_context(|| format!("input device not found: {configured}"))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn build_stream(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    samples: &Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream> {
    let error_callback = |error| eprintln!("audio input stream error: {error}");
    macro_rules! stream {
        ($sample:ty, $convert:expr) => {{
            let samples = Arc::clone(&samples);
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
        SampleFormat::U8 => stream!(u8, |sample: u8| {
            (f32::from(sample) / f32::from(u8::MAX)) * 2.0 - 1.0
        }),
        SampleFormat::U16 => stream!(u16, |sample: u16| {
            (f32::from(sample) / f32::from(u16::MAX)) * 2.0 - 1.0
        }),
        SampleFormat::U32 => stream!(u32, |sample: u32| {
            (sample as f32 / u32::MAX as f32) * 2.0 - 1.0
        }),
        SampleFormat::U64 => stream!(u64, |sample: u64| {
            (sample as f32 / u64::MAX as f32) * 2.0 - 1.0
        }),
        other => bail!("unsupported sample format {other}"),
    };
    result.context("failed to build input stream")
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn recommend_gates(current: &AudioGatesConfig, rms: f32, peak: f32) -> GateLevels {
    let defaults = AudioGatesConfig::default();
    GateLevels {
        min_record_ms: current.min_record_ms.max(defaults.min_record_ms),
        min_rms_energy: rounded_gate(
            (rms * 3.0)
                .max(defaults.min_rms_energy)
                .max(current.min_rms_energy * 0.75),
        ),
        min_peak_energy: rounded_gate(
            (peak * 2.5)
                .max(defaults.min_peak_energy)
                .max(current.min_peak_energy * 0.75),
        ),
    }
}

fn rounded_gate(value: f32) -> f32 {
    ((value.clamp(0.0, 1.0) * 100_000.0).ceil() / 100_000.0).max(0.00001)
}

#[allow(clippy::cast_precision_loss)]
fn energy(samples: &[f32]) -> (f32, f32) {
    let sum_squares = samples.iter().map(|sample| sample * sample).sum::<f32>();
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0, f32::max);
    ((sum_squares / samples.len() as f32).sqrt(), peak)
}

fn print_report(report: &CalibrationReport) {
    println!("Microphone calibration");
    println!("Device: {}", report.device);
    println!(
        "Sample: {} ms at {} Hz, {} channel(s)",
        report.measured.duration_ms, report.sample_rate, report.channels
    );
    println!("Measured RMS: {:.5}", report.measured.rms_energy);
    println!("Measured peak: {:.5}", report.measured.peak_energy);
    println!();
    println!("Current gates:");
    print_gates(&report.current);
    println!("Recommended gates:");
    print_gates(&report.recommended);
    if report.applied {
        if let Some(path) = &report.config_path {
            println!("Saved recommendations to {path}");
            println!("Restart skaldd if it is already running.");
        }
    } else {
        println!("Run with --apply to write these settings to config.");
    }
    println!("Recalibrate after changing microphones, input gain, room noise, or desk position.");
}

fn print_gates(gates: &GateLevels) {
    println!("  audio.gates.min_record_ms = {}", gates.min_record_ms);
    println!("  audio.gates.min_rms_energy = {:.5}", gates.min_rms_energy);
    println!(
        "  audio.gates.min_peak_energy = {:.5}",
        gates.min_peak_energy
    );
}

impl From<&AudioGatesConfig> for GateLevels {
    fn from(value: &AudioGatesConfig) -> Self {
        Self {
            min_record_ms: value.min_record_ms,
            min_rms_energy: value.min_rms_energy,
            min_peak_energy: value.min_peak_energy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.000_001, "{left} != {right}");
    }

    #[test]
    fn recommendations_stay_above_default_floor() {
        let current = AudioGatesConfig::default();
        let recommended = recommend_gates(&current, 0.0001, 0.0002);
        assert_close(recommended.min_rms_energy, current.min_rms_energy);
        assert_close(recommended.min_peak_energy, current.min_peak_energy);
    }

    #[test]
    fn recommendations_rise_with_noise_floor() {
        let current = AudioGatesConfig::default();
        let recommended = recommend_gates(&current, 0.01, 0.03);
        assert_close(recommended.min_rms_energy, 0.03);
        assert_close(recommended.min_peak_energy, 0.075);
    }
}
