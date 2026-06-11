use std::time::Instant;

use tracing::warn;
use voxline_core::{
    config::PathsConfig,
    protocol::{JobId, JobState, Response},
};

use crate::{
    injection,
    jobs::{AppState, elapsed_ms, emit_error, now_ms, ok_response, state_error, update_state},
};

pub(crate) struct DeliveredText {
    pub(crate) copied_to_clipboard: bool,
    pub(crate) paste_outcome: injection::PasteOutcome,
    pub(crate) clipboard_restored: bool,
}

pub(crate) async fn capture_active_target_async() -> Option<voxline_platform::TargetContext> {
    tokio::task::spawn_blocking(voxline_platform::capture_active_target)
        .await
        .ok()
        .flatten()
}

pub(crate) fn prefer_clipboard_for_target(
    target: Option<&voxline_platform::TargetContext>,
    paths: &PathsConfig,
) -> bool {
    target
        .and_then(|target| {
            voxline_core::apps::match_app_profile(
                paths,
                target.app_id.as_deref(),
                target.title.as_deref(),
            )
        })
        .and_then(|profile| profile.injection.prefer_clipboard_only)
        .unwrap_or(false)
}

pub(crate) async fn deliver_text_to_target(
    state: &AppState,
    job_id: &JobId,
    text: &str,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    prefer_clipboard_only: bool,
) -> Result<DeliveredText, String> {
    let clipboard_snapshot = if state.injection.restore_clipboard {
        tokio::task::spawn_blocking(voxline_platform::save_clipboard)
            .await
            .ok()
    } else {
        None
    };
    let copied_to_clipboard = copy_final_text(state, job_id, text).await?;
    let paste_outcome = if copied_to_clipboard {
        insert_if_safe(
            state,
            job_id,
            target_at_stop,
            started,
            prefer_clipboard_only,
        )
        .await
    } else {
        injection::PasteOutcome::disabled("clipboard output is disabled")
    };
    let clipboard_restored = if injection::should_restore_clipboard(
        state.injection.restore_clipboard,
        paste_outcome.paste_succeeded,
    ) && let Some(snapshot) = clipboard_snapshot
    {
        let delay_ms = state.injection.paste_delay_ms;
        tokio::task::spawn_blocking(move || voxline_platform::wait_for_clipboard(delay_ms))
            .await
            .ok();
        match tokio::task::spawn_blocking(move || voxline_platform::restore_clipboard(snapshot))
            .await
        {
            Ok(Ok(())) => true,
            Ok(Err(error)) => {
                warn!(%error, "failed to restore previous clipboard");
                false
            }
            Err(_) => false,
        }
    } else {
        false
    };
    Ok(DeliveredText {
        copied_to_clipboard,
        paste_outcome,
        clipboard_restored,
    })
}

pub(crate) async fn copy_final_text(
    state: &AppState,
    job_id: &JobId,
    text: &str,
) -> Result<bool, String> {
    if !state.injection.copy_to_clipboard {
        return Ok(false);
    }
    update_state(state, Some(job_id.clone()), JobState::Copying).await;
    let text = text.to_owned();
    let notifications_enabled = state.notifications.enabled;
    tokio::task::spawn_blocking(move || {
        voxline_platform::copy_to_clipboard(&text).map_err(|error| {
            let message = error.to_string();
            if notifications_enabled {
                voxline_platform::notify("VoxLine clipboard failed", &message);
            }
            message
        })
    })
    .await
    .map_err(|_| "clipboard copy task failed".to_string())??;
    Ok(true)
}

pub(crate) async fn insert_if_safe(
    state: &AppState,
    job_id: &JobId,
    target_at_stop: Option<voxline_platform::TargetContext>,
    started: Instant,
    prefer_clipboard_only: bool,
) -> injection::PasteOutcome {
    if prefer_clipboard_only {
        return handle_clipboard_fallback(
            state,
            job_id,
            injection::PasteOutcome::clipboard_only(
                "application profile prefers clipboard-only output",
                "paste_profile_clipboard_only",
            ),
        );
    }
    let target_at_start = state.target_at_start.lock().await.take();
    let target_before_paste = capture_active_target_async().await;
    let paste_backend = voxline_platform::paste_backend();
    if let Some(outcome) = injection::evaluate_paste_safety(
        &state.injection.auto_paste,
        paste_backend,
        target_at_start.as_ref(),
        target_at_stop.as_ref(),
        target_before_paste.as_ref(),
        elapsed_ms(started),
        state.injection.max_paste_age_ms,
    ) {
        return handle_clipboard_fallback(state, job_id, outcome);
    }
    update_state(state, Some(job_id.clone()), JobState::Injecting).await;
    let delay_ms = state.injection.paste_delay_ms;
    tokio::task::spawn_blocking(move || voxline_platform::wait_for_clipboard(delay_ms))
        .await
        .ok();
    let backend = paste_backend.expect("safety check passed");
    match tokio::task::spawn_blocking(move || voxline_platform::paste(backend)).await {
        Ok(Ok(())) => injection::PasteOutcome::succeeded(),
        Ok(Err(error)) => handle_clipboard_fallback(
            state,
            job_id,
            injection::PasteOutcome::failed_after_attempt(format!("paste failed: {error}")),
        ),
        Err(_) => handle_clipboard_fallback(
            state,
            job_id,
            injection::PasteOutcome::failed_after_attempt("paste task failed"),
        ),
    }
}

pub(crate) fn handle_clipboard_fallback(
    state: &AppState,
    job_id: &JobId,
    outcome: injection::PasteOutcome,
) -> injection::PasteOutcome {
    if injection::should_emit_clipboard_fallback_error(
        state.injection.fallback_to_clipboard_only,
        outcome.warning_code,
    ) {
        emit_error(
            state,
            Some(job_id.clone()),
            outcome.warning_code.expect("warning code checked"),
            &outcome.insertion_reason,
        );
    }
    if state.notifications.enabled
        && injection::should_notify_clipboard_only(
            state.injection.fallback_to_clipboard_only,
            state.injection.notify_on_clipboard_only,
            outcome.warning_code,
        )
    {
        voxline_platform::notify("VoxLine clipboard only", &outcome.insertion_reason);
    }
    outcome
}

pub(crate) async fn test_clipboard(request_id: String, state: &AppState) -> Response {
    let test_value = format!("VoxLine clipboard test {}", now_ms());
    let result = tokio::task::spawn_blocking(move || {
        let snapshot = voxline_platform::save_clipboard();
        let result = voxline_platform::copy_to_clipboard(&test_value)
            .and_then(|()| voxline_platform::read_clipboard())
            .and_then(|value| {
                if value == test_value {
                    Ok(())
                } else {
                    Err(voxline_platform::PlatformError::InvalidOutput {
                        tool: "clipboard",
                        message: "clipboard contents did not match".into(),
                    })
                }
            });
        let restore_result = voxline_platform::restore_clipboard(snapshot);
        result.and(restore_result)
    })
    .await;
    match result {
        Ok(Ok(())) => ok_response(request_id, state.status.read().await.clone()),
        Ok(Err(error)) => {
            state_error(
                request_id,
                state,
                "clipboard_test_failed",
                &error.to_string(),
            )
            .await
        }
        Err(_) => {
            state_error(
                request_id,
                state,
                "clipboard_test_failed",
                "clipboard test task failed",
            )
            .await
        }
    }
}

pub(crate) async fn test_paste(request_id: String, state: &AppState) -> Response {
    let Some(target) = capture_active_target_async().await else {
        return state_error(
            request_id,
            state,
            "paste_test_unavailable",
            "active target detection is unavailable",
        )
        .await;
    };
    let Some(backend) = voxline_platform::paste_backend() else {
        return state_error(
            request_id,
            state,
            "paste_test_unavailable",
            "no supported paste adapter is available",
        )
        .await;
    };
    if backend != voxline_platform::PasteBackend::Hyprland && target.is_terminal() {
        return state_error(
            request_id,
            state,
            "paste_test_unavailable",
            "terminal paste shortcuts vary; test paste in a graphical text field",
        )
        .await;
    }
    let delay_ms = state.injection.paste_delay_ms;
    let target_for_check = target.clone();
    let result = tokio::task::spawn_blocking(move || {
        let snapshot = voxline_platform::save_clipboard();
        voxline_platform::copy_to_clipboard("VoxLine paste test")?;
        voxline_platform::wait_for_clipboard(delay_ms);
        if voxline_platform::capture_active_target().as_ref() != Some(&target_for_check) {
            return Err(voxline_platform::PlatformError::InvalidOutput {
                tool: "paste",
                message: "active target changed before paste".into(),
            });
        }
        let paste_result = voxline_platform::paste(backend);
        voxline_platform::wait_for_clipboard(delay_ms);
        let restore_result = voxline_platform::restore_clipboard(snapshot);
        paste_result.and(restore_result)
    })
    .await;
    match result {
        Ok(Ok(())) => ok_response(request_id, state.status.read().await.clone()),
        Ok(Err(error)) => {
            state_error(request_id, state, "paste_test_failed", &error.to_string()).await
        }
        Err(_) => {
            state_error(
                request_id,
                state,
                "paste_test_failed",
                "paste test task failed",
            )
            .await
        }
    }
}
