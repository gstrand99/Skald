use std::time::Instant;

use thiserror::Error;
use voxline_core::{
    cleanup::DEFAULT_OPENROUTER_MODEL,
    config::{CleanupConfig, PathsConfig, SecretsConfig},
    secrets::{self, SecretError},
    snippet_templates::{self, TemplateError, TemplateFailureMode, TemplateSnippetMetadata},
};

use crate::openrouter::OpenRouterError;

#[derive(Debug, Error)]
pub enum TemplateExtractError {
    #[error("cleanup provider {0} is not supported for template extraction")]
    UnsupportedProvider(String),
    #[error("{0}")]
    Secret(#[from] SecretError),
    #[error("{0}")]
    OpenRouter(#[from] OpenRouterError),
    #[error("{0}")]
    Template(#[from] TemplateError),
}

#[derive(Debug, Clone)]
pub struct TemplateRenderOutcome {
    pub text: String,
    pub used_extraction: bool,
    pub failed: bool,
    pub extract_ms: u64,
}

pub async fn run_template_snippet(
    cleanup: &CleanupConfig,
    paths: &PathsConfig,
    secrets_config: &SecretsConfig,
    metadata: &TemplateSnippetMetadata,
    input: &str,
) -> Result<TemplateRenderOutcome, TemplateExtractError> {
    let started = Instant::now();
    if cleanup.provider != "openrouter" {
        return Err(TemplateExtractError::UnsupportedProvider(
            cleanup.provider.clone(),
        ));
    }
    let body = snippet_templates::load_template_body(paths, metadata)?;
    let api_key = secrets::lookup_openrouter_key(secrets_config)?;
    let model = if cleanup.model.trim().is_empty() {
        DEFAULT_OPENROUTER_MODEL
    } else {
        cleanup.model.as_str()
    };
    let system_prompt = snippet_templates::build_extraction_prompt(metadata);
    let response = crate::openrouter::complete_chat(
        &api_key,
        model,
        cleanup.temperature,
        cleanup.timeout_ms,
        &system_prompt,
        input,
    )
    .await?;
    let extracted = snippet_templates::parse_extraction_json(&response)?;
    let rendered = snippet_templates::render_template_snippet(metadata, &body, &extracted)?;
    Ok(TemplateRenderOutcome {
        text: rendered,
        used_extraction: true,
        failed: false,
        extract_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

pub fn raw_fallback_outcome(raw: impl Into<String>) -> TemplateRenderOutcome {
    TemplateRenderOutcome {
        text: raw.into(),
        used_extraction: true,
        failed: true,
        extract_ms: 0,
    }
}

#[must_use]
pub fn should_use_raw_fallback(metadata: &TemplateSnippetMetadata) -> bool {
    matches!(
        metadata.fallback.on_extract_failure,
        TemplateFailureMode::Raw
    )
}
