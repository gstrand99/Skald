use std::time::Instant;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use thiserror::Error;
use voxline_core::{
    cleanup::{DEFAULT_OPENROUTER_MODEL, validate_cleanup_output},
    config::{CleanupConfig, PathsConfig, SecretsConfig},
    secrets::{self, SecretError},
    styles::{self, StyleError},
};

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const OPENROUTER_HTTP_REFERER: &str = "https://github.com/gstrand99/VoxLine";
const OPENROUTER_X_TITLE: &str = "VoxLine";

#[derive(Debug, Error)]
pub enum CleanupError {
    #[error("cleanup provider {0} is not supported")]
    UnsupportedProvider(String),
    #[error("{0}")]
    Secret(#[from] SecretError),
    #[error("cleanup request failed: {0}")]
    Request(String),
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
    let cleaned = request_openrouter(
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

pub fn failed_fallback_outcome(raw: impl Into<String>) -> CleanupOutcome {
    CleanupOutcome {
        text: raw.into(),
        used: true,
        failed: true,
        cleanup_ms: 0,
    }
}

async fn request_openrouter(
    api_key: &str,
    model: &str,
    temperature: f32,
    timeout_ms: u64,
    system_prompt: &str,
    input: &str,
) -> Result<String, CleanupError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
        .map_err(|error| CleanupError::Request(error.to_string()))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|error| CleanupError::Request(error.to_string()))?,
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        HeaderName::from_static("http-referer"),
        HeaderValue::from_static(OPENROUTER_HTTP_REFERER),
    );
    headers.insert(
        HeaderName::from_static("x-title"),
        HeaderValue::from_static(OPENROUTER_X_TITLE),
    );
    let body = serde_json::json!({
        "model": model,
        "temperature": temperature,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": input}
        ]
    });
    let response = client
        .post(OPENROUTER_URL)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|error| CleanupError::Request(error.to_string()))?;
    let status = response.status();
    let payload = response
        .text()
        .await
        .map_err(|error| CleanupError::Request(error.to_string()))?;
    if !status.is_success() {
        return Err(CleanupError::Request(format!(
            "openrouter returned {status}"
        )));
    }
    let parsed: OpenRouterResponse = serde_json::from_str(&payload)
        .map_err(|error| CleanupError::InvalidResponse(error.to_string()))?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| CleanupError::InvalidResponse("empty completion".into()))?;
    Ok(content.trim().to_owned())
}

#[derive(Debug, Deserialize)]
struct OpenRouterResponse {
    choices: Vec<OpenRouterChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterMessage,
}

#[derive(Debug, Deserialize)]
struct OpenRouterMessage {
    content: String,
}
