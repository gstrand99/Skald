use std::{fs, sync::Arc, time::Instant};

use tracing::warn;
use voxline_core::{
    cleanup::{CleanupOverride, should_run_cleanup_with_voice_style},
    config::{CleanupConfig, Config, PathsConfig, SecretsConfig, VoiceCommandsConfig},
    protocol::{
        AsrBenchmark, AudioRecording, DaemonStatus, DictationResult, Event, JobId, JobState,
        ModelState, PROTOCOL_VERSION, PublicDictationResult, Response, Transcript,
    },
};

use crate::{
    asr, cleanup,
    delivery::{capture_active_target_async, deliver_text_to_target, prefer_clipboard_for_target},
    jobs::{
        AppState, CancelDecision, StopDecision, ToggleDecision, audio_error_response,
        cancel_decision, clear_per_job_context, data_response, data_response_with, elapsed_ms,
        emit_error, error_response, load_job_config_snapshot, now_ms, ok_response,
        reload_job_config, snapshot_job_config, state_error, stop_decision, toggle_decision,
        try_begin_job, update_preview_model_state, update_state,
    },
    template_extract,
};

pub(crate) async fn transcribe(
    request_id: String,
    state: &AppState,
    audio_path: std::path::PathBuf,
) -> Response {
    if try_begin_job(state, None, JobState::Transcribing)
        .await
        .is_err()
    {
        return state_error(request_id, state, "busy", "VoxLine is busy").await;
    }
    update_model_state(state, ModelState::Loading).await;
    match state.asr.transcribe(audio_path).await {
        Ok((transcript, benchmark)) => {
            update_model_state(state, ModelState::Ready).await;
            let status = update_state(state, None, JobState::Idle).await;
            data_response(request_id, status, None, Some(transcript), Some(benchmark))
        }
        Err(error) => asr_error_response(request_id, state, error).await,
    }
}

pub(crate) async fn asr_load(request_id: String, state: &AppState) -> Response {
    update_model_state(state, ModelState::Loading).await;
    match state.asr.load().await {
        Ok(model_load_ms) => {
            let status = update_model_state(state, ModelState::Ready).await;
            data_response(
                request_id,
                status,
                None,
                None,
                Some(AsrBenchmark {
                    model_load_ms,
                    transcribe_ms: 0,
                    audio_duration_ms: 0,
                }),
            )
        }
        Err(error) => asr_error_response(request_id, state, error).await,
    }
}

pub(crate) async fn asr_unload(request_id: String, state: &AppState) -> Response {
    match state.asr.unload().await {
        Ok(()) => {
            let status = update_model_state(state, ModelState::Unloaded).await;
            ok_response(request_id, status)
        }
        Err(error) => asr_error_response(request_id, state, error).await,
    }
}

pub(crate) async fn asr_restart(request_id: String, state: &AppState) -> Response {
    if let Err(error) = state.asr.unload().await {
        return asr_error_response(request_id, state, error).await;
    }
    asr_load(request_id, state).await
}

pub(crate) async fn update_model_state(state: &AppState, model_state: ModelState) -> DaemonStatus {
    let mut status = state.status.write().await;
    status.final_model_state.clone_from(&model_state);
    let snapshot = status.clone();
    let _ = state.events.send(Event::State {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        job_id: snapshot.active_job_id.clone(),
        job_state: snapshot.job_state.clone(),
        final_model_state: model_state,
    });
    snapshot
}

pub(crate) async fn asr_error_response(
    request_id: String,
    state: &AppState,
    error: asr::AsrError,
) -> Response {
    let message = error.to_string();
    update_model_state(
        state,
        ModelState::Failed {
            code: "asr_error".into(),
            message: message.clone(),
        },
    )
    .await;
    // `JobState::Failed` is reserved in the protocol but unused; pipeline errors reset to Idle.
    clear_per_job_context(state).await;
    let status = update_state(state, None, JobState::Idle).await;
    emit_error(state, None, "asr_error", &message);
    error_response(request_id, "asr_error", &message, Some(status))
}
pub(crate) async fn toggle(
    request_id: String,
    state: Arc<AppState>,
    cleanup: Option<CleanupOverride>,
    style: Option<String>,
    snippet: Option<String>,
) -> Response {
    let job_state = state.status.read().await.job_state.clone();
    match toggle_decision(&job_state) {
        ToggleDecision::Start => {
            if let Some(name) = snippet
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                return insert_snippet(request_id, &state, name.into()).await;
            }
            start(request_id, state, cleanup, style).await
        }
        ToggleDecision::Stop => stop(request_id, &state).await,
        ToggleDecision::Busy => state_error(request_id, &state, "busy", "VoxLine is busy").await,
    }
}

pub(crate) async fn start(
    request_id: String,
    state: Arc<AppState>,
    cleanup: Option<CleanupOverride>,
    style: Option<String>,
) -> Response {
    let job_id = JobId::new();
    if try_begin_job(&state, Some(job_id.clone()), JobState::Recording)
        .await
        .is_err()
    {
        return state_error(request_id, &state, "busy", "VoxLine is busy").await;
    }
    let config = Config::load_or_default().unwrap_or_default();
    let job_snapshot = snapshot_job_config(&config);
    *state.job_config.lock().await = Some(job_snapshot.clone());
    *state.cleanup_override.lock().await = cleanup;
    let target_at_start = capture_active_target_async().await;
    let active_app_profile = target_at_start.as_ref().and_then(|target| {
        voxline_core::apps::match_app_profile(
            &job_snapshot.paths,
            target.app_id.as_deref(),
            target.title.as_deref(),
        )
    });
    *state.target_at_start.lock().await = target_at_start;
    *state.active_app_profile.lock().await = active_app_profile;
    *state.style_override.lock().await = style;
    let preview_ring_buffer_seconds = state
        .preview
        .is_enabled()
        .then_some(job_snapshot.preview_ring_buffer_seconds);
    match state
        .audio
        .start(job_id.clone(), preview_ring_buffer_seconds)
        .await
    {
        Ok(()) => {
            if let (Some(tap), Some(preview_asr)) =
                (state.audio.current_tap(), state.preview_asr.clone())
            {
                let state_for_preview = Arc::clone(&state);
                state
                    .preview
                    .start(
                        job_id.clone(),
                        tap,
                        preview_asr,
                        Arc::new(move |model_state| {
                            let state = Arc::clone(&state_for_preview);
                            tokio::spawn(async move {
                                update_preview_model_state(&state, model_state).await;
                            });
                        }),
                    )
                    .await;
            }
            let status = state.status.read().await.clone();
            ok_response(request_id, status)
        }
        Err(error) => audio_error_response(request_id, &state, error).await,
    }
}

pub(crate) async fn stop(request_id: String, state: &AppState) -> Response {
    let started = Instant::now();
    let target_at_stop = capture_active_target_async().await;
    let job_id = {
        let status = state.status.read().await;
        if stop_decision(&status.job_state) != StopDecision::Allowed {
            drop(status);
            return state_error(
                request_id,
                state,
                "no_active_recording",
                "there is no active recording",
            )
            .await;
        }
        status
            .active_job_id
            .clone()
            .expect("recording has a job id")
    };
    state.preview.stop().await;
    if let Some(preview_asr) = &state.preview_asr {
        let _ = preview_asr.unload().await;
    }
    if state.preview.is_enabled() {
        update_preview_model_state(state, ModelState::Unloaded).await;
    }
    update_state(state, Some(job_id.clone()), JobState::Stopping).await;
    match state.audio.stop(job_id).await {
        Ok(recording) => {
            finish_dictation(
                request_id,
                state,
                recording,
                target_at_stop,
                started,
                true,
                false,
            )
            .await
        }
        Err(error) => audio_error_response(request_id, state, error).await,
    }
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn finish_dictation(
    request_id: String,
    state: &AppState,
    recording: AudioRecording,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    attempt_paste: bool,
    retain_audio_file: bool,
) -> Response {
    let _audio_cleanup = TemporaryAudio::new(
        recording.job_id.clone(),
        recording.wav_path.clone(),
        state.privacy.store_audio || retain_audio_file,
    );
    if recording.truncated && state.notifications.enabled {
        voxline_platform::notify(
            "VoxLine",
            "Recording stopped at the configured maximum length",
        );
    }
    if !recording.speech_detected {
        if state.notifications.enabled && state.audio_gates.notify_on_no_speech {
            voxline_platform::notify("VoxLine", "No speech detected");
        }
        clear_per_job_context(state).await;
        let status = update_state(state, None, JobState::Idle).await;
        return error_response(
            request_id,
            "no_speech",
            "recording was too short or quiet to transcribe",
            Some(status),
        );
    }

    update_model_state(state, ModelState::Loading).await;
    update_state(
        state,
        Some(recording.job_id.clone()),
        JobState::Transcribing,
    )
    .await;
    let (transcript, benchmark) = match state.asr.transcribe(recording.wav_path.clone()).await {
        Ok(result) => result,
        Err(error) => return asr_error_response(request_id, state, error).await,
    };
    update_model_state(state, ModelState::Ready).await;
    if transcript.text.trim().is_empty() {
        if state.notifications.enabled {
            voxline_platform::notify("VoxLine", "No speech recognized");
        }
        clear_per_job_context(state).await;
        let status = update_state(state, None, JobState::Idle).await;
        return error_response(
            request_id,
            "empty_transcript",
            "transcription produced no usable text",
            Some(status),
        );
    }

    let cleanup_override = state.cleanup_override.lock().await.take();
    let style_override = state.style_override.lock().await.take();
    let active_app_profile = state.active_app_profile.lock().await.take();
    let job_snapshot = state
        .job_config
        .lock()
        .await
        .take()
        .unwrap_or_else(load_job_config_snapshot);
    let cleanup_config = job_snapshot.cleanup;
    let paths_config = job_snapshot.paths;
    let secrets_config = job_snapshot.secrets;
    let cleanup_enabled = job_snapshot.cleanup_enabled;
    let voice_commands_config = job_snapshot.voice_commands;
    let prefer_clipboard_only = active_app_profile
        .as_ref()
        .and_then(|profile| profile.injection.prefer_clipboard_only)
        .unwrap_or(false);
    let voice_command_outcome = apply_voice_commands(
        &voice_commands_config,
        &paths_config,
        &transcript.text,
        state.privacy.log_transcripts,
    );
    let voice_style_override = voice_command_outcome.voice_style;
    let raw_text = voice_command_outcome.raw_text;
    if let Some(name) = voice_command_outcome.insert_snippet_only {
        return deliver_snippet_from_job(
            request_id,
            state,
            SnippetJobContext {
                recording,
                benchmark,
                target_at_stop,
                started,
                paths_config,
                name,
                prefer_clipboard_only,
            },
        )
        .await;
    }
    if let Some(name) = voice_command_outcome.template_snippet {
        return deliver_template_from_job(
            request_id,
            state,
            TemplateJobContext {
                recording,
                benchmark,
                target_at_stop,
                started,
                cleanup_config,
                paths_config,
                secrets_config,
                name,
                input: raw_text.clone(),
                prefer_clipboard_only,
            },
        )
        .await;
    }
    if raw_text.trim().is_empty() {
        clear_per_job_context(state).await;
        let status = update_state(state, None, JobState::Idle).await;
        return error_response(
            request_id,
            "empty_transcript",
            "transcription produced no usable text after command stripping",
            Some(status),
        );
    }
    let routing = voxline_core::routing::resolve_cleanup_routing(
        style_override.as_deref(),
        voice_style_override.as_deref(),
        cleanup_override,
        cleanup_enabled,
        &cleanup_config.default_style,
        active_app_profile.as_ref(),
    );
    let cleanup_outcome = if should_run_cleanup_with_voice_style(
        routing.cleanup_enabled,
        cleanup_override,
        &raw_text,
        cleanup_config.skip_if_word_count_below,
        voice_style_override.is_some(),
    ) {
        update_state(state, Some(recording.job_id.clone()), JobState::Cleaning).await;
        let cleanup_started = Instant::now();
        match cleanup::run_cleanup(
            &cleanup_config,
            &paths_config,
            &secrets_config,
            &routing.style_name,
            routing.app_prompt.as_deref(),
            &raw_text,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                warn!(%error, "cleanup failed; falling back to raw transcript");
                if cleanup_config.fallback_to_raw_on_error {
                    cleanup::failed_fallback_outcome(raw_text, elapsed_ms(cleanup_started))
                } else {
                    let status = update_state(state, None, JobState::Idle).await;
                    return error_response(
                        request_id,
                        "cleanup_error",
                        &error.to_string(),
                        Some(status),
                    );
                }
            }
        }
    } else {
        cleanup::passthrough_outcome(raw_text)
    };
    let final_text = cleanup_outcome.text.clone();
    let mut final_transcript = transcript.clone();
    final_transcript.text = final_text.clone();
    {
        let mut status = state.status.write().await;
        status.cleanup_enabled = cleanup_enabled;
    }

    let delivery = match deliver_text_to_target(
        state,
        &recording.job_id,
        &final_text,
        target_at_stop,
        started,
        routing.prefer_clipboard_only || !attempt_paste,
    )
    .await
    {
        Ok(delivery) => delivery,
        Err(message) => {
            let status = update_state(state, None, JobState::Idle).await;
            emit_error(state, Some(recording.job_id), "clipboard_error", &message);
            return error_response(request_id, "clipboard_error", &message, Some(status));
        }
    };
    let copied_to_clipboard = delivery.copied_to_clipboard;
    let paste_outcome = delivery.paste_outcome;
    let clipboard_restored = delivery.clipboard_restored;

    let result = DictationResult {
        job_id: recording.job_id.clone(),
        transcript: final_transcript.clone(),
        benchmark: benchmark.clone(),
        total_ms: elapsed_ms(started),
        copied_to_clipboard,
        pasted: paste_outcome.paste_succeeded,
        paste_attempted: paste_outcome.paste_attempted,
        paste_succeeded: paste_outcome.paste_succeeded,
        clipboard_restored,
        cleanup_used: cleanup_outcome.used,
        cleanup_failed: cleanup_outcome.failed,
        snippet_used: None,
        insertion_reason: paste_outcome.insertion_reason.clone(),
    };
    let _ = state.events.send(Event::Result {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        result: PublicDictationResult::from_result(
            &result,
            state.privacy.emit_transcript_in_events,
        ),
    });
    if state.notifications.enabled {
        voxline_platform::notify(
            "VoxLine",
            if paste_outcome.paste_succeeded {
                "Paste command sent"
            } else if copied_to_clipboard {
                "Transcript copied to clipboard"
            } else {
                "Transcription complete"
            },
        );
    }
    update_state(state, Some(recording.job_id.clone()), JobState::Done).await;
    let status = update_state(state, None, JobState::Idle).await;
    data_response_with(
        request_id,
        status,
        Some(recording),
        Some(final_transcript),
        Some(benchmark),
        if cleanup_outcome.used {
            Some(cleanup_outcome.cleanup_ms)
        } else {
            None
        },
        Some(result),
    )
}

pub(crate) async fn test_openrouter(request_id: String, state: &AppState) -> Response {
    let (cleanup_config, paths_config, secrets_config, _, _) = reload_job_config();
    if cleanup_config.provider != "openrouter" {
        return state_error(
            request_id,
            state,
            "openrouter_test_unavailable",
            "cleanup provider is not set to openrouter",
        )
        .await;
    }
    let routing = voxline_core::routing::resolve_cleanup_routing(
        None,
        None,
        None,
        cleanup_config.enabled,
        &cleanup_config.default_style,
        None,
    );
    match cleanup::run_cleanup(
        &cleanup_config,
        &paths_config,
        &secrets_config,
        &routing.style_name,
        routing.app_prompt.as_deref(),
        "VoxLine OpenRouter test",
    )
    .await
    {
        Ok(outcome) => {
            let status = state.status.read().await.clone();
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
                cleaned_text: Some(outcome.text),
                cleanup_ms: Some(outcome.cleanup_ms),
                dictation: None,
                model_bench_results: None,
            }
        }
        Err(error) => {
            state_error(
                request_id,
                state,
                "openrouter_test_failed",
                &error.to_string(),
            )
            .await
        }
    }
}

pub(crate) async fn cleanup_preview(
    request_id: String,
    state: &AppState,
    text: String,
    style: Option<String>,
) -> Response {
    let (cleanup_config, paths_config, secrets_config, cleanup_enabled, _) = reload_job_config();
    if !cleanup_enabled && cleanup_config.provider == "none" {
        return state_error(
            request_id,
            state,
            "cleanup_disabled",
            "cleanup is disabled; run voxline cleanup enable openrouter",
        )
        .await;
    }
    let routing = voxline_core::routing::resolve_cleanup_routing(
        style.as_deref(),
        None,
        None,
        cleanup_enabled,
        &cleanup_config.default_style,
        None,
    );
    match cleanup::run_cleanup(
        &cleanup_config,
        &paths_config,
        &secrets_config,
        &routing.style_name,
        routing.app_prompt.as_deref(),
        &text,
    )
    .await
    {
        Ok(outcome) => {
            let status = state.status.read().await.clone();
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
                cleaned_text: Some(outcome.text),
                cleanup_ms: Some(outcome.cleanup_ms),
                dictation: None,
                model_bench_results: None,
            }
        }
        Err(error) => {
            state_error(
                request_id,
                state,
                "cleanup_preview_failed",
                &error.to_string(),
            )
            .await
        }
    }
}
pub(crate) struct SnippetJobContext {
    recording: AudioRecording,
    benchmark: AsrBenchmark,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    paths_config: PathsConfig,
    name: String,
    prefer_clipboard_only: bool,
}

pub(crate) struct VoiceCommandOutcome {
    raw_text: String,
    voice_style: Option<String>,
    insert_snippet_only: Option<String>,
    template_snippet: Option<String>,
}

pub(crate) fn apply_voice_commands(
    voice_commands_config: &VoiceCommandsConfig,
    paths_config: &PathsConfig,
    transcript: &str,
    log_transcripts: bool,
) -> VoiceCommandOutcome {
    let mut outcome = VoiceCommandOutcome {
        raw_text: transcript.to_owned(),
        voice_style: None,
        insert_snippet_only: None,
        template_snippet: None,
    };
    if !voice_commands_config.enabled {
        return outcome;
    }
    let Ok(registry) = voxline_core::commands::build_command_registry(paths_config) else {
        tracing::debug!("voice command registry unavailable");
        return outcome;
    };
    let Some(parsed) = voxline_core::commands::parse_voice_command(
        voice_commands_config,
        &registry,
        transcript.trim(),
    ) else {
        if log_transcripts {
            tracing::debug!(transcript, "no voice command matched transcript");
        }
        return outcome;
    };
    outcome.raw_text.clone_from(&parsed.remainder);
    match parsed.target {
        voxline_core::commands::CommandTarget::Style { name } => {
            outcome.voice_style = Some(name);
        }
        voxline_core::commands::CommandTarget::Snippet { name } => {
            match voxline_core::snippets::snippet_kind(paths_config, &name) {
                Ok(voxline_core::snippets::SnippetKind::Insert)
                    if outcome.raw_text.trim().is_empty() =>
                {
                    outcome.insert_snippet_only = Some(name);
                }
                Ok(voxline_core::snippets::SnippetKind::Template)
                    if !outcome.raw_text.trim().is_empty() =>
                {
                    outcome.template_snippet = Some(name);
                }
                _ => {}
            }
        }
    }
    outcome
}

pub(crate) struct TemplateJobContext {
    recording: AudioRecording,
    benchmark: AsrBenchmark,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    cleanup_config: CleanupConfig,
    paths_config: PathsConfig,
    secrets_config: SecretsConfig,
    name: String,
    input: String,
    prefer_clipboard_only: bool,
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn deliver_template_from_job(
    request_id: String,
    state: &AppState,
    job: TemplateJobContext,
) -> Response {
    let TemplateJobContext {
        recording,
        benchmark,
        target_at_stop,
        started,
        cleanup_config,
        paths_config,
        secrets_config,
        name,
        input,
        prefer_clipboard_only,
    } = job;
    let metadata =
        match voxline_core::snippet_templates::load_template_metadata(&paths_config, &name) {
            Ok(metadata) => metadata,
            Err(error) => {
                let status = update_state(state, None, JobState::Idle).await;
                return error_response(
                    request_id,
                    "template_error",
                    &error.to_string(),
                    Some(status),
                );
            }
        };
    update_state(state, Some(recording.job_id.clone()), JobState::Cleaning).await;
    let render_outcome = match template_extract::run_template_snippet(
        &cleanup_config,
        &paths_config,
        &secrets_config,
        &metadata,
        &input,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            warn!(%error, "template extraction failed");
            if template_extract::should_use_raw_fallback(&metadata) {
                template_extract::raw_fallback_outcome(input)
            } else {
                let status = update_state(state, None, JobState::Idle).await;
                return error_response(
                    request_id,
                    "template_error",
                    &error.to_string(),
                    Some(status),
                );
            }
        }
    };
    let delivery = match deliver_text_to_target(
        state,
        &recording.job_id,
        &render_outcome.text,
        target_at_stop,
        started,
        prefer_clipboard_only,
    )
    .await
    {
        Ok(delivery) => delivery,
        Err(message) => {
            let status = update_state(state, None, JobState::Idle).await;
            emit_error(
                state,
                Some(recording.job_id.clone()),
                "clipboard_error",
                &message,
            );
            return error_response(request_id, "clipboard_error", &message, Some(status));
        }
    };
    let final_transcript = Transcript {
        text: render_outcome.text.clone(),
        language: None,
        duration_ms: Some(recording.duration_ms),
        segments: Vec::new(),
    };
    let result = DictationResult {
        job_id: recording.job_id.clone(),
        transcript: final_transcript.clone(),
        benchmark: benchmark.clone(),
        total_ms: elapsed_ms(started),
        copied_to_clipboard: delivery.copied_to_clipboard,
        pasted: delivery.paste_outcome.paste_succeeded,
        paste_attempted: delivery.paste_outcome.paste_attempted,
        paste_succeeded: delivery.paste_outcome.paste_succeeded,
        clipboard_restored: delivery.clipboard_restored,
        cleanup_used: render_outcome.used_extraction,
        cleanup_failed: render_outcome.failed,
        snippet_used: Some(name),
        insertion_reason: delivery.paste_outcome.insertion_reason.clone(),
    };
    let _ = state.events.send(Event::Result {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        result: PublicDictationResult::from_result(
            &result,
            state.privacy.emit_transcript_in_events,
        ),
    });
    if state.notifications.enabled {
        voxline_platform::notify(
            "VoxLine",
            if delivery.paste_outcome.paste_succeeded {
                "Template paste command sent"
            } else if delivery.copied_to_clipboard {
                "Template copied to clipboard"
            } else {
                "Template ready"
            },
        );
    }
    update_state(state, Some(recording.job_id.clone()), JobState::Done).await;
    let status = update_state(state, None, JobState::Idle).await;
    data_response(
        request_id,
        status,
        Some(recording),
        Some(final_transcript),
        Some(benchmark),
    )
}

pub(crate) async fn template_preview(
    request_id: String,
    state: &AppState,
    name: String,
    text: String,
) -> Response {
    let (cleanup_config, paths_config, secrets_config, cleanup_enabled, _) = reload_job_config();
    if !cleanup_enabled || cleanup_config.provider != "openrouter" {
        return state_error(
            request_id,
            state,
            "template_preview_unavailable",
            "template extraction requires enabled openrouter cleanup",
        )
        .await;
    }
    let metadata =
        match voxline_core::snippet_templates::load_template_metadata(&paths_config, &name) {
            Ok(metadata) => metadata,
            Err(error) => {
                return state_error(request_id, state, "template_error", &error.to_string()).await;
            }
        };
    match template_extract::run_template_snippet(
        &cleanup_config,
        &paths_config,
        &secrets_config,
        &metadata,
        &text,
    )
    .await
    {
        Ok(outcome) => {
            let status = state.status.read().await.clone();
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
                cleaned_text: Some(outcome.text),
                cleanup_ms: Some(outcome.extract_ms),
                dictation: None,
                model_bench_results: None,
            }
        }
        Err(error) => {
            state_error(
                request_id,
                state,
                "template_preview_failed",
                &error.to_string(),
            )
            .await
        }
    }
}

pub(crate) async fn deliver_snippet_from_job(
    request_id: String,
    state: &AppState,
    job: SnippetJobContext,
) -> Response {
    let SnippetJobContext {
        recording,
        benchmark,
        target_at_stop,
        started,
        paths_config,
        name,
        prefer_clipboard_only,
    } = job;
    let snippet_text = match voxline_core::snippets::load_snippet_content(&paths_config, &name) {
        Ok(snippet_text) => snippet_text,
        Err(error) => {
            let status = update_state(state, None, JobState::Idle).await;
            return error_response(
                request_id,
                "snippet_error",
                &error.to_string(),
                Some(status),
            );
        }
    };
    let delivery = match deliver_text_to_target(
        state,
        &recording.job_id,
        &snippet_text,
        target_at_stop,
        started,
        prefer_clipboard_only,
    )
    .await
    {
        Ok(delivery) => delivery,
        Err(message) => {
            let status = update_state(state, None, JobState::Idle).await;
            emit_error(
                state,
                Some(recording.job_id.clone()),
                "clipboard_error",
                &message,
            );
            return error_response(request_id, "clipboard_error", &message, Some(status));
        }
    };
    let final_transcript = Transcript {
        text: snippet_text.clone(),
        language: None,
        duration_ms: Some(recording.duration_ms),
        segments: Vec::new(),
    };
    let result = DictationResult {
        job_id: recording.job_id.clone(),
        transcript: final_transcript.clone(),
        benchmark: benchmark.clone(),
        total_ms: elapsed_ms(started),
        copied_to_clipboard: delivery.copied_to_clipboard,
        pasted: delivery.paste_outcome.paste_succeeded,
        paste_attempted: delivery.paste_outcome.paste_attempted,
        paste_succeeded: delivery.paste_outcome.paste_succeeded,
        clipboard_restored: delivery.clipboard_restored,
        cleanup_used: false,
        cleanup_failed: false,
        snippet_used: Some(name),
        insertion_reason: delivery.paste_outcome.insertion_reason.clone(),
    };
    let _ = state.events.send(Event::Result {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        result: PublicDictationResult::from_result(
            &result,
            state.privacy.emit_transcript_in_events,
        ),
    });
    if state.notifications.enabled {
        voxline_platform::notify(
            "VoxLine",
            if delivery.paste_outcome.paste_succeeded {
                "Snippet paste command sent"
            } else if delivery.copied_to_clipboard {
                "Snippet copied to clipboard"
            } else {
                "Snippet ready"
            },
        );
    }
    update_state(state, Some(recording.job_id.clone()), JobState::Done).await;
    let status = update_state(state, None, JobState::Idle).await;
    data_response(
        request_id,
        status,
        Some(recording),
        Some(final_transcript),
        Some(benchmark),
    )
}

pub(crate) async fn insert_snippet(request_id: String, state: &AppState, name: String) -> Response {
    let job_id = JobId::new();
    if try_begin_job(state, Some(job_id.clone()), JobState::Copying)
        .await
        .is_err()
    {
        return state_error(request_id, state, "busy", "VoxLine is busy").await;
    }
    let started = Instant::now();
    let target_at_insert = capture_active_target_async().await;
    *state.target_at_start.lock().await = target_at_insert.clone();
    let paths_config = Config::load_or_default()
        .map(|config| config.paths)
        .unwrap_or_default();
    let prefer_clipboard_only =
        prefer_clipboard_for_target(target_at_insert.as_ref(), &paths_config);
    let content = match voxline_core::snippets::load_snippet_content(&paths_config, &name) {
        Ok(content) => content,
        Err(error) => {
            clear_per_job_context(state).await;
            let status = update_state(state, None, JobState::Idle).await;
            return error_response(
                request_id,
                "snippet_error",
                &error.to_string(),
                Some(status),
            );
        }
    };
    let delivery = match deliver_text_to_target(
        state,
        &job_id,
        &content,
        target_at_insert,
        started,
        prefer_clipboard_only,
    )
    .await
    {
        Ok(delivery) => delivery,
        Err(message) => {
            let status = update_state(state, None, JobState::Idle).await;
            emit_error(state, Some(job_id), "clipboard_error", &message);
            return error_response(request_id, "clipboard_error", &message, Some(status));
        }
    };
    let copied_to_clipboard = delivery.copied_to_clipboard;
    let paste_outcome = delivery.paste_outcome;
    let clipboard_restored = delivery.clipboard_restored;
    let transcript = Transcript {
        text: content.clone(),
        language: None,
        duration_ms: None,
        segments: Vec::new(),
    };
    let result = DictationResult {
        job_id: job_id.clone(),
        transcript: transcript.clone(),
        benchmark: AsrBenchmark {
            model_load_ms: 0,
            transcribe_ms: 0,
            audio_duration_ms: 0,
        },
        total_ms: elapsed_ms(started),
        copied_to_clipboard,
        pasted: paste_outcome.paste_succeeded,
        paste_attempted: paste_outcome.paste_attempted,
        paste_succeeded: paste_outcome.paste_succeeded,
        clipboard_restored,
        cleanup_used: false,
        cleanup_failed: false,
        snippet_used: Some(name),
        insertion_reason: paste_outcome.insertion_reason.clone(),
    };
    let _ = state.events.send(Event::Result {
        protocol_version: PROTOCOL_VERSION,
        timestamp_ms: now_ms(),
        result: PublicDictationResult::from_result(
            &result,
            state.privacy.emit_transcript_in_events,
        ),
    });
    if state.notifications.enabled {
        voxline_platform::notify(
            "VoxLine",
            if paste_outcome.paste_succeeded {
                "Snippet paste command sent"
            } else if copied_to_clipboard {
                "Snippet copied to clipboard"
            } else {
                "Snippet ready"
            },
        );
    }
    update_state(state, Some(job_id.clone()), JobState::Done).await;
    let status = update_state(state, None, JobState::Idle).await;
    data_response(request_id, status, None, Some(transcript), None)
}

struct TemporaryAudio {
    job_id: JobId,
    path: std::path::PathBuf,
    retain: bool,
}

impl TemporaryAudio {
    fn new(job_id: JobId, path: std::path::PathBuf, retain: bool) -> Self {
        Self {
            job_id,
            path,
            retain,
        }
    }
}

impl Drop for TemporaryAudio {
    fn drop(&mut self) {
        if self.retain {
            return;
        }
        if let Err(error) = fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            warn!(
                job_id = %self.job_id.0,
                kind = ?error.kind(),
                "failed to delete temporary audio"
            );
        }
    }
}

pub(crate) async fn cancel(request_id: String, state: &AppState) -> Response {
    let job_id = {
        let status = state.status.read().await;
        match cancel_decision(&status.job_state) {
            CancelDecision::Allowed => status
                .active_job_id
                .clone()
                .expect("recording has a job id"),
            CancelDecision::NoActiveRecording => {
                drop(status);
                return state_error(
                    request_id,
                    state,
                    "no_active_recording",
                    "there is no active recording",
                )
                .await;
            }
            CancelDecision::CannotCancel => {
                let job_state = status.job_state.clone();
                drop(status);
                return state_error(
                    request_id,
                    state,
                    "cannot_cancel",
                    &format!("cannot cancel while job is {job_state:?}"),
                )
                .await;
            }
        }
    };
    state.preview.stop().await;
    if let Some(preview_asr) = &state.preview_asr {
        let _ = preview_asr.unload().await;
    }
    if state.preview.is_enabled() {
        update_preview_model_state(state, ModelState::Unloaded).await;
    }
    match state.audio.cancel(job_id.clone()).await {
        Ok(()) => {
            clear_per_job_context(state).await;
            update_state(state, Some(job_id), JobState::Cancelled).await;
            let status = update_state(state, None, JobState::Idle).await;
            ok_response(request_id, status)
        }
        Err(error) => audio_error_response(request_id, state, error).await,
    }
}
