use voxline_core::config::AutoPasteMode;
use voxline_platform::{PasteBackend, TargetContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteOutcome {
    pub paste_attempted: bool,
    pub paste_succeeded: bool,
    pub insertion_reason: String,
    pub warning_code: Option<&'static str>,
}

impl PasteOutcome {
    #[must_use]
    pub fn disabled(reason: impl Into<String>) -> Self {
        Self {
            paste_attempted: false,
            paste_succeeded: false,
            insertion_reason: reason.into(),
            warning_code: None,
        }
    }

    #[must_use]
    pub fn clipboard_only(reason: impl Into<String>, warning_code: &'static str) -> Self {
        Self {
            paste_attempted: false,
            paste_succeeded: false,
            insertion_reason: reason.into(),
            warning_code: Some(warning_code),
        }
    }

    #[must_use]
    pub fn succeeded() -> Self {
        Self {
            paste_attempted: true,
            paste_succeeded: true,
            insertion_reason: "paste command sent to the stable active target".into(),
            warning_code: None,
        }
    }

    #[must_use]
    pub fn failed_after_attempt(reason: impl Into<String>) -> Self {
        Self {
            paste_attempted: true,
            paste_succeeded: false,
            insertion_reason: reason.into(),
            warning_code: Some("paste_failed"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasteUnsafeReason {
    UnsupportedSession,
    TerminalUnsafe,
    Stale,
    TargetChanged,
}

impl PasteUnsafeReason {
    const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedSession => "paste_unsupported_session",
            Self::TerminalUnsafe => "paste_terminal_unsafe",
            Self::Stale => "paste_unsafe_stale",
            Self::TargetChanged => "paste_unsafe_target_changed",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::UnsupportedSession => "paste is unsupported in this desktop session",
            Self::TerminalUnsafe => {
                "terminal paste shortcuts vary; transcript left on the clipboard"
            }
            Self::Stale => "transcript exceeded the maximum paste age",
            Self::TargetChanged => "active target changed or could not be verified",
        }
    }
}

fn check_paste_safety(
    auto_paste: &AutoPasteMode,
    paste_backend: Option<PasteBackend>,
    target_at_start: Option<&TargetContext>,
    target_at_stop: Option<&TargetContext>,
    target_before_paste: Option<&TargetContext>,
    elapsed_ms: u64,
    max_paste_age_ms: u64,
) -> Result<(), PasteUnsafeReason> {
    let Some(backend) = paste_backend else {
        return Err(PasteUnsafeReason::UnsupportedSession);
    };
    if backend != PasteBackend::Hyprland
        && target_before_paste
            .as_ref()
            .is_some_and(|target| target.is_terminal())
    {
        return Err(PasteUnsafeReason::TerminalUnsafe);
    }
    if elapsed_ms > max_paste_age_ms {
        return Err(PasteUnsafeReason::Stale);
    }
    if *auto_paste == AutoPasteMode::Safe
        && !targets_are_stable(target_at_start, target_at_stop, target_before_paste)
    {
        return Err(PasteUnsafeReason::TargetChanged);
    }
    Ok(())
}

#[must_use]
fn same_target(a: &TargetContext, b: &TargetContext) -> bool {
    a.backend == b.backend && a.id == b.id && a.app_id == b.app_id
}

#[must_use]
pub fn evaluate_paste_safety(
    auto_paste: &AutoPasteMode,
    paste_backend: Option<PasteBackend>,
    target_at_start: Option<&TargetContext>,
    target_at_stop: Option<&TargetContext>,
    target_before_paste: Option<&TargetContext>,
    elapsed_ms: u64,
    max_paste_age_ms: u64,
) -> Option<PasteOutcome> {
    if *auto_paste == AutoPasteMode::Off {
        return Some(PasteOutcome::disabled("automatic paste is disabled"));
    }
    match check_paste_safety(
        auto_paste,
        paste_backend,
        target_at_start,
        target_at_stop,
        target_before_paste,
        elapsed_ms,
        max_paste_age_ms,
    ) {
        Ok(()) => None,
        Err(reason) => Some(PasteOutcome::clipboard_only(
            reason.message(),
            reason.code(),
        )),
    }
}

#[must_use]
pub fn should_notify_clipboard_only(
    fallback_to_clipboard_only: bool,
    notify_on_clipboard_only: bool,
    warning_code: Option<&str>,
) -> bool {
    warning_code.is_some() && fallback_to_clipboard_only && notify_on_clipboard_only
}

#[must_use]
pub fn should_emit_clipboard_fallback_error(
    fallback_to_clipboard_only: bool,
    warning_code: Option<&str>,
) -> bool {
    !fallback_to_clipboard_only && warning_code.is_some()
}

#[must_use]
pub fn should_restore_clipboard(restore_clipboard: bool, paste_succeeded: bool) -> bool {
    restore_clipboard && paste_succeeded
}

#[must_use]
pub fn targets_are_stable(
    start: Option<&TargetContext>,
    stop: Option<&TargetContext>,
    before_paste: Option<&TargetContext>,
) -> bool {
    match (start, stop, before_paste) {
        (Some(start), Some(stop), Some(before_paste)) => {
            same_target(start, stop) && same_target(stop, before_paste)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxline_platform::TargetBackend;

    fn target(id: &str) -> TargetContext {
        TargetContext {
            backend: TargetBackend::Hyprland,
            id: id.into(),
            app_id: Some("app".into()),
            title: Some("title".into()),
        }
    }

    fn target_with_title(id: &str, title: &str) -> TargetContext {
        TargetContext {
            title: Some(title.into()),
            ..target(id)
        }
    }

    fn terminal_target() -> TargetContext {
        TargetContext {
            backend: TargetBackend::Sway,
            id: "1".into(),
            app_id: Some("kitty".into()),
            title: Some("shell".into()),
        }
    }

    #[test]
    fn requires_the_same_known_target_at_all_three_points() {
        assert!(targets_are_stable(
            Some(&target("0x1")),
            Some(&target("0x1")),
            Some(&target("0x1"))
        ));
        assert!(!targets_are_stable(
            Some(&target("0x1")),
            Some(&target("0x2")),
            Some(&target("0x2"))
        ));
        assert!(!targets_are_stable(None, None, None));
    }

    #[test]
    fn title_only_changes_are_stable() {
        assert!(targets_are_stable(
            Some(&target_with_title("0x1", "doc.md")),
            Some(&target_with_title("0x1", "doc.md*")),
            Some(&target_with_title("0x1", "doc.md — saved"))
        ));
    }

    #[test]
    fn identity_changes_are_unstable() {
        assert!(!targets_are_stable(
            Some(&target("0x1")),
            Some(&TargetContext {
                app_id: Some("other-app".into()),
                ..target("0x1")
            }),
            Some(&TargetContext {
                app_id: Some("other-app".into()),
                ..target("0x1")
            })
        ));
        assert!(!targets_are_stable(
            Some(&target("0x1")),
            Some(&target("0x2")),
            Some(&target("0x2"))
        ));
    }

    #[test]
    fn classifies_unsafe_paste_reasons() {
        let stable = (
            Some(&target("0x1")),
            Some(&target("0x1")),
            Some(&target("0x1")),
        );
        assert_eq!(
            evaluate_paste_safety(
                &AutoPasteMode::Safe,
                None,
                stable.0,
                stable.1,
                stable.2,
                0,
                5_000
            )
            .expect("outcome")
            .warning_code,
            Some("paste_unsupported_session")
        );
        assert_eq!(
            evaluate_paste_safety(
                &AutoPasteMode::Safe,
                Some(PasteBackend::Wtype),
                Some(&terminal_target()),
                Some(&terminal_target()),
                Some(&terminal_target()),
                0,
                5_000
            )
            .expect("outcome")
            .warning_code,
            Some("paste_terminal_unsafe")
        );
        assert_eq!(
            evaluate_paste_safety(
                &AutoPasteMode::Safe,
                Some(PasteBackend::Hyprland),
                stable.0,
                stable.1,
                stable.2,
                6_000,
                5_000
            )
            .expect("outcome")
            .warning_code,
            Some("paste_unsafe_stale")
        );
        assert_eq!(
            evaluate_paste_safety(
                &AutoPasteMode::Safe,
                Some(PasteBackend::Hyprland),
                Some(&target("0x1")),
                Some(&target("0x2")),
                Some(&target("0x2")),
                0,
                5_000
            )
            .expect("outcome")
            .warning_code,
            Some("paste_unsafe_target_changed")
        );
        assert!(
            evaluate_paste_safety(
                &AutoPasteMode::Safe,
                Some(PasteBackend::Hyprland),
                stable.0,
                stable.1,
                stable.2,
                0,
                5_000
            )
            .is_none()
        );
    }

    #[test]
    fn always_mode_enforces_staleness_but_not_target_stability() {
        let unstable = (
            Some(&target("0x1")),
            Some(&target("0x2")),
            Some(&target("0x2")),
        );
        assert!(
            evaluate_paste_safety(
                &AutoPasteMode::Always,
                Some(PasteBackend::Hyprland),
                unstable.0,
                unstable.1,
                unstable.2,
                0,
                5_000
            )
            .is_none()
        );
        assert_eq!(
            evaluate_paste_safety(
                &AutoPasteMode::Always,
                Some(PasteBackend::Hyprland),
                unstable.0,
                unstable.1,
                unstable.2,
                6_000,
                5_000
            )
            .expect("outcome")
            .warning_code,
            Some("paste_unsafe_stale")
        );
    }

    #[test]
    fn auto_paste_off_is_not_a_fallback_warning() {
        let outcome = evaluate_paste_safety(
            &AutoPasteMode::Off,
            Some(PasteBackend::Hyprland),
            None,
            None,
            None,
            0,
            5_000,
        )
        .expect("outcome");
        assert!(outcome.warning_code.is_none());
        assert!(!outcome.paste_attempted);
    }

    #[test]
    fn fallback_error_emission_follows_config() {
        assert!(!should_emit_clipboard_fallback_error(
            true,
            Some("paste_unsafe_target_changed")
        ));
        assert!(should_emit_clipboard_fallback_error(
            false,
            Some("paste_unsafe_target_changed")
        ));
        assert!(!should_emit_clipboard_fallback_error(false, None));
    }

    #[test]
    fn clipboard_only_notification_follows_config() {
        assert!(should_notify_clipboard_only(
            true,
            true,
            Some("paste_unsafe_target_changed")
        ));
        assert!(!should_notify_clipboard_only(
            true,
            false,
            Some("paste_unsafe_target_changed")
        ));
        assert!(!should_notify_clipboard_only(
            false,
            true,
            Some("paste_unsafe_target_changed")
        ));
    }

    #[test]
    fn restore_only_after_successful_paste() {
        assert!(should_restore_clipboard(true, true));
        assert!(!should_restore_clipboard(true, false));
        assert!(!should_restore_clipboard(false, true));
    }
}
