use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use thiserror::Error;

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const OPENROUTER_HTTP_REFERER: &str = "https://github.com/gstrand99/VoxLine";
const OPENROUTER_X_TITLE: &str = "VoxLine";

#[derive(Debug, Error)]
pub enum OpenRouterError {
    #[error("openrouter request failed: {0}")]
    Request(String),
    #[error("openrouter response was invalid: {0}")]
    InvalidResponse(String),
}

pub async fn complete_chat(
    api_key: &str,
    model: &str,
    temperature: f32,
    timeout_ms: u64,
    system_prompt: &str,
    input: &str,
) -> Result<String, OpenRouterError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
        .map_err(|error| OpenRouterError::Request(error.to_string()))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|error| OpenRouterError::Request(error.to_string()))?,
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
        .map_err(|error| OpenRouterError::Request(error.to_string()))?;
    let status = response.status();
    let payload = response
        .text()
        .await
        .map_err(|error| OpenRouterError::Request(error.to_string()))?;
    if !status.is_success() {
        return Err(OpenRouterError::Request(format!(
            "openrouter returned {status}"
        )));
    }
    let parsed: OpenRouterResponse = serde_json::from_str(&payload)
        .map_err(|error| OpenRouterError::InvalidResponse(error.to_string()))?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| OpenRouterError::InvalidResponse("empty completion".into()))?;
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
