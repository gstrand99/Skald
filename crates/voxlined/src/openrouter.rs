use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use reqwest::{
    StatusCode,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
};
use serde::Deserialize;
use thiserror::Error;

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const OPENROUTER_HTTP_REFERER: &str = "https://github.com/gstrand99/VoxLine";
const OPENROUTER_X_TITLE: &str = "VoxLine";
const RETRY_BACKOFF: Duration = Duration::from_millis(250);

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

#[derive(Debug, Error)]
pub enum OpenRouterError {
    #[error("openrouter request failed: {0}")]
    Request(String),
    #[error("openrouter response was invalid: {0}")]
    InvalidResponse(String),
}

#[derive(Debug)]
enum AttemptError {
    Http { status: StatusCode },
    Request(String),
    InvalidResponse(String),
}

impl From<AttemptError> for OpenRouterError {
    fn from(error: AttemptError) -> Self {
        match error {
            AttemptError::Http { status } => Self::Request(format!("openrouter returned {status}")),
            AttemptError::Request(message) => Self::Request(message),
            AttemptError::InvalidResponse(message) => Self::InvalidResponse(message),
        }
    }
}

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .build()
            .expect("reqwest client should build")
    })
}

fn should_retry(status: StatusCode) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

pub async fn complete_chat(
    api_key: &str,
    model: &str,
    temperature: f32,
    timeout_ms: u64,
    system_prompt: &str,
    input: &str,
) -> Result<String, OpenRouterError> {
    let budget = Duration::from_millis(timeout_ms);
    let started = Instant::now();
    match send_chat_request(
        api_key,
        model,
        temperature,
        system_prompt,
        input,
        budget.saturating_sub(started.elapsed()),
    )
    .await
    {
        Ok(content) => Ok(content),
        Err(AttemptError::Http { status }) if should_retry(status) => {
            if started.elapsed().saturating_add(RETRY_BACKOFF) >= budget {
                return Err(AttemptError::Http { status }.into());
            }
            tokio::time::sleep(RETRY_BACKOFF).await;
            let remaining = budget.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(AttemptError::Http { status }.into());
            }
            send_chat_request(api_key, model, temperature, system_prompt, input, remaining)
                .await
                .map_err(Into::into)
        }
        Err(error) => Err(error.into()),
    }
}

async fn send_chat_request(
    api_key: &str,
    model: &str,
    temperature: f32,
    system_prompt: &str,
    input: &str,
    timeout: Duration,
) -> Result<String, AttemptError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|error| AttemptError::Request(error.to_string()))?,
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
    let response = http_client()
        .post(OPENROUTER_URL)
        .timeout(timeout)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|error| AttemptError::Request(error.to_string()))?;
    let status = response.status();
    let payload = response
        .text()
        .await
        .map_err(|error| AttemptError::Request(error.to_string()))?;
    if !status.is_success() {
        return Err(AttemptError::Http { status });
    }
    let parsed: OpenRouterResponse = serde_json::from_str(&payload)
        .map_err(|error| AttemptError::InvalidResponse(error.to_string()))?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| AttemptError::InvalidResponse("empty completion".into()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_retry_only_rate_limit_and_server_errors() {
        assert!(should_retry(StatusCode::TOO_MANY_REQUESTS));
        assert!(should_retry(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(should_retry(StatusCode::BAD_GATEWAY));
        assert!(!should_retry(StatusCode::BAD_REQUEST));
        assert!(!should_retry(StatusCode::UNAUTHORIZED));
        assert!(!should_retry(StatusCode::NOT_FOUND));
    }
}
