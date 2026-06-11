use std::time::Instant;

use thiserror::Error;
use voxline_core::{
    cleanup::{DEFAULT_OPENROUTER_MODEL, validate_cleanup_output},
    config::{CleanupConfig, PathsConfig, SecretsConfig},
    secrets::{self, SecretError},
    styles::{self, StyleError},
};

#[derive(Debug, Error)]
pub enum CleanupError {
    #[error("cleanup provider {0} is not supported")]
    UnsupportedProvider(String),
    #[error("{0}")]
    Secret(#[from] SecretError),
    #[error("{0}")]
    OpenRouter(#[from] crate::openrouter::OpenRouterError),
    #[error("cleanup response was invalid: {0}")]
    InvalidResponse(String),
    #[error("{0}")]
    Style(#[from] StyleError),
}

#[derive(Debug, Clone)]
pub struct CleanupOutcome {
    pub text: String,
    pub used: bool,
    pub failed: bool,
    pub cleanup_ms: u64,
}

pub async fn run_cleanup(
    cleanup: &CleanupConfig,
    paths: &PathsConfig,
    secrets_config: &SecretsConfig,
    style_name: &str,
    app_prompt: Option<&str>,
    input: &str,
) -> Result<CleanupOutcome, CleanupError> {
    let started = Instant::now();
    if cleanup.provider != "openrouter" {
        return Err(CleanupError::UnsupportedProvider(cleanup.provider.clone()));
    }
    let api_key = secrets::lookup_openrouter_key(secrets_config)?;
    let model = if cleanup.model.trim().is_empty() {
        DEFAULT_OPENROUTER_MODEL
    } else {
        cleanup.model.as_str()
    };
    let mut system_prompt = styles::load_style_prompt(paths, style_name)?;
    if let Some(layer) = app_prompt.map(str::trim).filter(|layer| !layer.is_empty()) {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(layer);
    }
    let cleaned = crate::openrouter::complete_chat(
        &api_key,
        model,
        cleanup.temperature,
        cleanup.timeout_ms,
        &system_prompt,
        input,
    )
    .await?;
    if !validate_cleanup_output(input, &cleaned) {
        return Err(CleanupError::InvalidResponse(
            "model output failed validation".into(),
        ));
    }
    Ok(CleanupOutcome {
        text: cleaned,
        used: true,
        failed: false,
        cleanup_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

pub fn passthrough_outcome(text: impl Into<String>) -> CleanupOutcome {
    CleanupOutcome {
        text: text.into(),
        used: false,
        failed: false,
        cleanup_ms: 0,
    }
}

pub fn failed_fallback_outcome(raw: impl Into<String>, cleanup_ms: u64) -> CleanupOutcome {
    CleanupOutcome {
        text: raw.into(),
        used: true,
        failed: true,
        cleanup_ms,
    }
}
