//! Small helpers around `wiremock` for LLM integration tests.

#![allow(dead_code)]

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Wrap an assistant `content` string into a full OpenAI chat-completions JSON
/// envelope, as `ReqwestClient` expects.
pub fn envelope(content: &str) -> serde_json::Value {
    json!({
        "id": "mock",
        "object": "chat.completion",
        "created": 0,
        "model": "mock",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop"
        }]
    })
}

/// Register a one-shot mock that returns `content` wrapped in an envelope.
pub async fn expect_ok(server: &MockServer, content: &str) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(content)))
        .expect(1)
        .mount(server)
        .await;
}

/// Register a one-shot mock that returns a non-2xx status with a body.
pub async fn expect_status(server: &MockServer, status: u16, body: &str) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .expect(1)
        .mount(server)
        .await;
}
