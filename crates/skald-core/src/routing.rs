use crate::{apps::AppProfile, cleanup::CleanupOverride, styles};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupRouting {
    pub style_name: String,
    pub cleanup_enabled: bool,
    pub app_prompt: Option<String>,
    pub prefer_clipboard_only: bool,
}

#[must_use]
pub fn resolve_cleanup_routing(
    cli_style: Option<&str>,
    voice_style: Option<&str>,
    cli_cleanup: Option<CleanupOverride>,
    global_cleanup_enabled: bool,
    global_default_style: &str,
    app_profile: Option<&AppProfile>,
) -> CleanupRouting {
    let style_name = resolve_style_name(cli_style, voice_style, app_profile, global_default_style);
    let cleanup_enabled = resolve_cleanup_enabled(cli_cleanup, global_cleanup_enabled, app_profile);
    let app_prompt = app_profile
        .and_then(|profile| profile.prompt.as_ref())
        .map(|prompt| prompt.trim())
        .filter(|prompt| !prompt.is_empty())
        .map(ToOwned::to_owned);
    let prefer_clipboard_only = app_profile
        .and_then(|profile| profile.injection.prefer_clipboard_only)
        .unwrap_or(false);
    CleanupRouting {
        style_name,
        cleanup_enabled,
        app_prompt,
        prefer_clipboard_only,
    }
}

#[must_use]
pub fn resolve_style_name(
    cli_style: Option<&str>,
    voice_style: Option<&str>,
    app_profile: Option<&AppProfile>,
    global_default_style: &str,
) -> String {
    if let Some(style) = cli_style.map(str::trim).filter(|name| !name.is_empty()) {
        return style.to_owned();
    }
    if let Some(style) = voice_style.map(str::trim).filter(|name| !name.is_empty()) {
        return style.to_owned();
    }
    if let Some(style) = app_profile
        .and_then(|profile| profile.default_style.as_ref())
        .map(|style| style.trim())
        .filter(|style| !style.is_empty())
    {
        return style.to_owned();
    }
    styles::resolve_style_name(None, global_default_style)
}

#[must_use]
pub fn resolve_cleanup_enabled(
    cli_cleanup: Option<CleanupOverride>,
    global_cleanup_enabled: bool,
    app_profile: Option<&AppProfile>,
) -> bool {
    match cli_cleanup {
        Some(CleanupOverride::Force) => true,
        Some(CleanupOverride::Disable) => false,
        None => app_profile
            .and_then(|profile| profile.cleanup.enabled)
            .unwrap_or(global_cleanup_enabled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::{AppProfile, AppProfileCleanup, AppProfileInjection};

    fn slack_profile() -> AppProfile {
        AppProfile {
            name: "Slack".into(),
            default_style: Some("casual".into()),
            match_process: vec!["slack".into()],
            match_app_id: vec![],
            prompt: Some("Chat formatting.".into()),
            cleanup: AppProfileCleanup { enabled: None },
            injection: AppProfileInjection {
                prefer_clipboard_only: None,
            },
        }
    }

    #[test]
    fn cli_style_overrides_app_default() {
        let routing = resolve_cleanup_routing(
            Some("professional"),
            None,
            None,
            true,
            "default",
            Some(&slack_profile()),
        );
        assert_eq!(routing.style_name, "professional");
    }

    #[test]
    fn voice_style_used_without_cli_override() {
        let routing = resolve_cleanup_routing(
            None,
            Some("professional"),
            None,
            true,
            "default",
            Some(&slack_profile()),
        );
        assert_eq!(routing.style_name, "professional");
    }

    #[test]
    fn app_style_used_without_cli_override() {
        let routing =
            resolve_cleanup_routing(None, None, None, true, "default", Some(&slack_profile()));
        assert_eq!(routing.style_name, "casual");
    }

    #[test]
    fn app_cleanup_disable_overrides_global_enable() {
        let mut profile = slack_profile();
        profile.cleanup.enabled = Some(false);
        let routing = resolve_cleanup_routing(None, None, None, true, "default", Some(&profile));
        assert!(!routing.cleanup_enabled);
    }
}
