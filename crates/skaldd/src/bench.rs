use std::{
    fs,
    time::{Duration, Instant},
};

use skald_core::{
    config::Config,
    diagnostics::{
        DiagnosticOutcome, DiagnosticSource, Measurement, PreviewMetrics, TimingMetrics,
    },
    protocol::{AsrBenchCandidate, JobId, JobState, ModelBenchResult, PROTOCOL_VERSION, Response},
};

use crate::{
    audio,
    delivery::capture_active_target_async,
    diagnostics,
    dictation::finish_dictation,
    jobs::{
        AppState, audio_error_response, clear_per_job_context, error_response, snapshot_job_config,
        state_error, success_response, try_begin_job, update_state,
    },
};

pub(crate) async fn bench_dictation(
    request_id: String,
    state: &AppState,
    audio_path: std::path::PathBuf,
    cleanup: Option<skald_core::cleanup::CleanupOverride>,
    attempt_paste: bool,
) -> Response {
    if try_begin_job(state, None, JobState::Transcribing)
        .await
        .is_err()
    {
        return state_error(request_id, state, "busy", "Skald is busy").await;
    }
    let config = Config::load_or_default().unwrap_or_default();
    *state.job_config.lock().await = Some(snapshot_job_config(&config));
    let recording = match audio::recording_from_existing_wav(
        &audio_path,
        &config.audio.gates,
        config.audio.target_sample_rate,
    ) {
        Ok(recording) => recording,
        Err(error) => {
            clear_per_job_context(state).await;
            let status = update_state(state, None, JobState::Idle).await;
            return error_response(
                request_id,
                "bench_audio_invalid",
                &error.to_string(),
                Some(status),
            );
        }
    };
    if let Some(cleanup) = cleanup {
        *state.cleanup_override.lock().await = Some(cleanup);
    }
    let target_at_stop = if attempt_paste {
        capture_active_target_async().await
    } else {
        None
    };
    finish_dictation(
        request_id,
        state,
        recording,
        target_at_stop,
        Instant::now(),
        attempt_paste,
        true,
    )
    .await
}

pub(crate) async fn diagnostics_benchmark(
    request_id: String,
    state: &AppState,
    audio_path: std::path::PathBuf,
) -> Response {
    if try_begin_job(state, None, JobState::Transcribing)
        .await
        .is_err()
    {
        return state_error(request_id, state, "busy", "Skald is busy").await;
    }
    let config = Config::load_or_default().unwrap_or_default();
    let recording = match audio::recording_from_existing_wav(
        &audio_path,
        &config.audio.gates,
        config.audio.target_sample_rate,
    ) {
        Ok(recording) => recording,
        Err(error) => {
            let status = update_state(state, None, JobState::Idle).await;
            return error_response(
                request_id,
                "bench_audio_invalid",
                &error.to_string(),
                Some(status),
            );
        }
    };
    let started = Instant::now();
    let context = diagnostics::context(&config, acceleration_backend());
    let mut record = diagnostics::empty_record(DiagnosticSource::Benchmark, context);
    record.timings.recording_duration_ms = Measurement::value(recording.duration_ms);
    record.timings.audio_duration_ms = Measurement::value(recording.duration_ms);
    match state.asr.transcribe(recording.wav_path).await {
        Ok((_transcript, benchmark)) => {
            record.timings = TimingMetrics {
                recording_duration_ms: Measurement::value(recording.duration_ms),
                stop_to_finalization_ms: Measurement::value(
                    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                ),
                model_load_ms: Measurement::value(benchmark.model_load_ms),
                asr_inference_ms: Measurement::value(benchmark.transcribe_ms),
                audio_duration_ms: Measurement::value(benchmark.audio_duration_ms),
                asr_real_time_factor_milli: if benchmark.audio_duration_ms == 0 {
                    Measurement::Unavailable
                } else {
                    Measurement::value(
                        benchmark
                            .transcribe_ms
                            .saturating_mul(1_000)
                            .saturating_div(benchmark.audio_duration_ms),
                    )
                },
                end_to_end_ms: Measurement::value(
                    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                ),
                ..TimingMetrics::default()
            };
            record.preview = PreviewMetrics {
                configured_enabled: config.preview.enabled,
                effective_enabled: config.preview_enabled_effective(),
                inference_ms: Measurement::NotAttempted,
            };
        }
        Err(error) => {
            record.outcome = DiagnosticOutcome {
                status: "failed".into(),
                error_code: Some("asr_error".into()),
            };
            record.timings.stop_to_finalization_ms =
                Measurement::value(started.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
            record.timings.end_to_end_ms =
                Measurement::value(started.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
            record.timings.asr_inference_ms = Measurement::failed("asr_error");
            tracing::warn!(%error, "diagnostics benchmark ASR failed");
        }
    }
    let enabled = state.diagnostics.lock().await.enabled();
    if enabled {
        diagnostics::record(state, record).await;
    }
    let snapshot = state.diagnostics.lock().await.snapshot();
    let status = update_state(state, None, JobState::Idle).await;
    Response {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: true,
        status: Some(status),
        recording: None,
        transcript: None,
        benchmark: None,
        error: None,
        session_environment: None,
        cleaned_text: None,
        cleanup_ms: None,
        dictation: None,
        model_bench_results: None,
        diagnostics: Some(snapshot),
    }
}

fn acceleration_backend() -> &'static str {
    if cfg!(feature = "asr-whisper-rs-cuda") {
        "cuda"
    } else {
        "cpu"
    }
}

pub(crate) async fn setup_record(
    request_id: String,
    state: &AppState,
    seconds: u64,
    output_path: std::path::PathBuf,
) -> Response {
    let job_id = JobId::new();
    if try_begin_job(state, Some(job_id.clone()), JobState::Recording)
        .await
        .is_err()
    {
        return state_error(request_id, state, "busy", "Skald is busy").await;
    }
    if let Err(error) = state.audio.start(job_id.clone(), None).await {
        return audio_error_response(request_id, state, error).await;
    }
    tokio::time::sleep(Duration::from_secs(seconds.max(1))).await;
    state.preview.stop().await;
    let recording = match state.audio.stop(job_id).await {
        Ok(recording) => recording,
        Err(error) => {
            let _ = update_state(state, None, JobState::Idle).await;
            return audio_error_response(request_id, state, error).await;
        }
    };
    let status = update_state(state, None, JobState::Idle).await;
    if !recording.speech_detected {
        return error_response(
            request_id,
            "no_speech",
            "recording was too short or quiet to transcribe",
            Some(status),
        );
    }
    if let Some(parent) = output_path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return error_response(
            request_id,
            "setup_record_failed",
            &error.to_string(),
            Some(status),
        );
    }
    if let Err(error) = fs::copy(&recording.wav_path, &output_path) {
        return error_response(
            request_id,
            "setup_record_failed",
            &error.to_string(),
            Some(status),
        );
    }
    let _ = fs::remove_file(&recording.wav_path);
    let mut saved = recording;
    saved.wav_path = output_path;
    success_response(request_id, status, Some(saved))
}

pub(crate) async fn bench_model_compare(
    request_id: String,
    state: &AppState,
    audio_path: std::path::PathBuf,
    candidates: Vec<AsrBenchCandidate>,
    include_cold_load: bool,
) -> Response {
    if try_begin_job(state, None, JobState::Transcribing)
        .await
        .is_err()
    {
        return state_error(request_id, state, "busy", "Skald is busy").await;
    }
    if candidates.is_empty() {
        let status = update_state(state, None, JobState::Idle).await;
        return error_response(
            request_id,
            "bench_no_candidates",
            "no models were provided for comparison",
            Some(status),
        );
    }
    let base_config = Config::load_or_default().unwrap_or_default().asr;
    let mut results = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let mut asr_config = base_config.clone();
        asr_config.model_path = candidate.model_path.display().to_string();
        asr_config.gpu = candidate.gpu;
        asr_config.lifecycle.mode = "on_demand".into();
        asr_config.lifecycle.idle_unload_seconds = 0;

        // `cold_load_ms` is only populated after an explicit unload+reload; otherwise 0.
        let cold_load_ms = if include_cold_load {
            let _ = state.asr.unload().await;
            match state.asr.reload(asr_config.clone()).await {
                Ok(ms) => ms,
                Err(error) => {
                    results.push(ModelBenchResult {
                        model_id: candidate.model_id.clone(),
                        cold_load_ms: 0,
                        warm_transcribe_ms: 0,
                        audio_duration_ms: 0,
                        transcript_text: String::new(),
                        error: Some(error.to_string()),
                    });
                    continue;
                }
            }
        } else {
            if let Err(error) = state.asr.reload(asr_config.clone()).await {
                results.push(ModelBenchResult {
                    model_id: candidate.model_id.clone(),
                    cold_load_ms: 0,
                    warm_transcribe_ms: 0,
                    audio_duration_ms: 0,
                    transcript_text: String::new(),
                    error: Some(error.to_string()),
                });
                continue;
            }
            0
        };

        match state.asr.transcribe(audio_path.clone()).await {
            Ok((transcript, benchmark)) => {
                results.push(ModelBenchResult {
                    model_id: candidate.model_id,
                    cold_load_ms,
                    warm_transcribe_ms: benchmark.transcribe_ms,
                    audio_duration_ms: benchmark.audio_duration_ms,
                    transcript_text: transcript.text,
                    error: None,
                });
            }
            Err(error) => {
                results.push(ModelBenchResult {
                    model_id: candidate.model_id,
                    cold_load_ms,
                    warm_transcribe_ms: 0,
                    audio_duration_ms: 0,
                    transcript_text: String::new(),
                    error: Some(error.to_string()),
                });
            }
        }
        let _ = state.asr.unload().await;
    }
    let _ = state.asr.reload(base_config).await;
    let status = update_state(state, None, JobState::Idle).await;
    Response {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: true,
        status: Some(status),
        recording: None,
        transcript: None,
        benchmark: None,
        error: None,
        session_environment: None,
        cleaned_text: None,
        cleanup_ms: None,
        dictation: None,
        model_bench_results: Some(results),
        diagnostics: None,
    }
}
