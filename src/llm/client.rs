//! LLM client abstraction and the production `reqwest`-based implementation.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::StatusCode;
use tokio_util::sync::CancellationToken;

use super::error::LlmError;
use super::types::{ChatRequest, ChatResponse};

/// Abstraction over a chat-completions endpoint, so Reflexion can be tested
/// against an in-process mock.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a chat request, return the assistant's `content` string.
    /// Errors map to `LlmError` variants. Cancellation via `cancel` returns
    /// `LlmError::Cancelled`.
    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<String, LlmError>;
}

/// Production client using `reqwest` against an OpenAI-compatible endpoint.
///
/// `base_url` is expected to include the API path prefix (e.g. `/v1`) —
/// see `Config::llm.base_url`.
pub struct ReqwestClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    request_timeout: Duration,
}

impl ReqwestClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        request_timeout: Duration,
    ) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(request_timeout)
            .build()
            .map_err(LlmError::Network)?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            api_key: api_key.into(),
            request_timeout,
        })
    }

    fn map_reqwest_err(&self, e: reqwest::Error) -> LlmError {
        if e.is_timeout() {
            LlmError::Timeout(self.request_timeout)
        } else {
            LlmError::Network(e)
        }
    }
}

#[async_trait]
impl LlmClient for ReqwestClient {
    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<String, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let send = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send();

        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError::Cancelled),
            r = send => r.map_err(|e| self.map_reqwest_err(e))?,
        };

        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(LlmError::Unauthorized);
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_secs);
            return Err(LlmError::RateLimited(retry_after));
        }
        if status.is_server_error() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Server {
                status: status.as_u16(),
                body,
            });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::InvalidResponse(format!(
                "unexpected status {status}: {body}"
            )));
        }

        let parsed: ChatResponse = resp
            .json()
            .await
            .map_err(|e| LlmError::InvalidResponse(format!("decode body: {e}")))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| LlmError::InvalidResponse("no choices in response".into()))?;
        Ok(content)
    }
}
