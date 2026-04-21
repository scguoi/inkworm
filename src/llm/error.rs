//! Error type for the LLM client layer.

use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("network error: {0}")]
    Network(reqwest::Error),
    #[error("request timed out after {0:?}")]
    Timeout(Duration),
    #[error("unauthorized (check API key)")]
    Unauthorized,
    #[error("rate limited (retry after {0:?})")]
    RateLimited(Option<Duration>),
    #[error("server error {status}: {body}")]
    Server { status: u16, body: String },
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_contains_status_and_body_for_server_error() {
        let e = LlmError::Server {
            status: 503,
            body: "overloaded".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("503"));
        assert!(msg.contains("overloaded"));
    }

    #[test]
    fn unauthorized_has_actionable_message() {
        let e = LlmError::Unauthorized;
        assert!(format!("{e}").to_lowercase().contains("api key"));
    }
}
