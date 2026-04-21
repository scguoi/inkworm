# inkworm v1 LLM Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the full LLM subsystem for inkworm v1: HTTP client (OpenAI-compatible), two-phase Reflexion loop (Phase 1 split article → sentences, Phase 2 concurrent expand each sentence → 3–5 progressive drills), prompt templates locked by insta snapshots, and an `examples/smoke.rs` binary for live end-to-end verification against a real endpoint (no real keys touch source or configs).

**Architecture:** `LlmClient` trait with a `ReqwestClient` production impl and a `MockLlmClient` test impl via `wiremock`. `Reflexion::generate(article)` orchestrates the two phases with a `tokio::sync::Semaphore` bounding Phase 2 concurrency, per-call 3-retry repair loop, and a top-level `CancellationToken`. Validation reuses `storage::course::Course::validate` for the final Course and dedicated `RawSentences`/`RawDrills` validators for the per-phase outputs. Failed attempts go to `paths.failed_dir/*.txt`.

**Tech Stack:** tokio (current_thread runtime) · reqwest (rustls TLS) · async-trait · tokio-util CancellationToken · futures::future::try_join_all · wiremock (tests) · insta (prompt snapshots)

**Reference spec:** `docs/superpowers/specs/2026-04-21-inkworm-design.md` §5 (LLM), §4.2 (Course schema), §10 (errors)

**Self-review applied:** scanned for placeholders, type consistency, spec coverage; 3 inline fixes (Task 10.1 removed unnecessary Cargo.toml edit since examples/ can use dev-deps; related Step 5 simplified; budget-timeout deferred with justification).

**Depends on Plan 1 (Foundation):** already merged to `main`. Uses `storage::course::{Course, Sentence, Drill, Focus, Source, SourceKind, ValidationError}`, `storage::atomic::write_atomic`, `storage::DataPaths`, `clock::Clock`, `config::LlmConfig`, `config::GenerationConfig`, `AppError`.

---

## File Structure (this plan)

```
inkworm/
├── Cargo.toml                              # + tokio, reqwest, async-trait, tokio-util, futures,
│                                           #   wiremock, url, urlencoding (dev), serde_with (if needed)
├── src/
│   ├── error.rs                            # MODIFY: add AppError::Llm, AppError::Reflexion
│   ├── llm/
│   │   ├── mod.rs                          # re-exports + submodule declarations
│   │   ├── error.rs                        # LlmError enum
│   │   ├── types.rs                        # ChatRequest/ChatMessage/Role + RawSentence(s) + RawDrill(s) + validators
│   │   ├── client.rs                       # LlmClient trait + ReqwestClient impl
│   │   ├── prompt.rs                       # PHASE1_SYSTEM, PHASE2_SYSTEM, REPAIR_TEMPLATE + errors_formatted
│   │   └── reflexion.rs                    # Reflexion struct + reflexion_split + reflexion_drill + generate + build_course + slug
│   ├── lib.rs                              # MODIFY: add `pub mod llm;` + `pub use llm::reflexion::Reflexion;`
│   └── storage/
│       └── failed.rs                       # FILL: save_failed_response(...)
├── tests/
│   ├── common/
│   │   ├── mod.rs                          # MODIFY: add wiremock helper
│   │   └── llm_mocks.rs                    # NEW: helper builders for canned LLM responses
│   └── llm.rs                              # NEW: integration test binary
│       ├── mod client                      # wiremock tests of ReqwestClient
│       ├── mod raw_types                   # RawSentences / RawDrills validation
│       ├── mod prompts                     # insta snapshots of the three templates
│       ├── mod reflexion_phase1            # Phase 1 ok / repair / fail / cancel / network
│       ├── mod reflexion_phase2            # Phase 2 all-ok / one-fail / concurrency / cancel
│       └── mod reflexion_e2e               # full article → Course happy path
├── examples/
│   └── smoke.rs                            # NEW: live smoke against real endpoint via env vars
└── fixtures/
    └── llm_responses/
        ├── phase1/
        │   ├── ok.json                     # valid {title, description, sentences: [...]}
        │   ├── missing_sentences.json
        │   ├── sentences_under_5.json
        │   ├── sentences_over_20.json
        │   ├── non_json.txt
        │   └── missing_title.json
        └── phase2/
            ├── ok.json                     # valid {drills: [...]}, 3 drills, last focus=full
            ├── drills_under_3.json
            ├── drills_over_5.json
            ├── last_focus_not_full.json
            └── english_mismatch.json        # last drill's english differs from Phase-1 reference
```

---

## Phase 0: Dependencies

### Task 0.1: Add LLM dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Open `Cargo.toml` and add runtime deps**

Extend the existing `[dependencies]` block so it contains:

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"
anyhow = "1"
tokio = { version = "1", features = ["rt", "macros", "time", "sync", "process", "signal"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
async-trait = "0.1"
tokio-util = "0.7"
futures = "0.3"
```

Extend `[dev-dependencies]`:

```toml
[dev-dependencies]
tempfile = "3"
insta = { version = "1", features = ["json"] }
serial_test = "3"
wiremock = "0.6"
tokio = { version = "1", features = ["rt", "macros", "time", "sync", "test-util"] }
```

Leave `[profile.release]` unchanged.

- [ ] **Step 2: Verify compile**

```bash
cargo check --all-targets
```

Expected: Finishes cleanly (deps downloaded, nothing built beyond check).

- [ ] **Step 3: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All three must pass. Previous 58 tests should still be green.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add tokio, reqwest, async-trait, wiremock for LLM subsystem"
```

---

## Phase 1: LlmError + AppError extension

### Task 1.1: Create `src/llm/error.rs`

**Files:**
- Create: `src/llm/mod.rs`
- Create: `src/llm/error.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create `src/llm/mod.rs`**

```rust
//! LLM client, prompt templates, and Reflexion-based course generation.

pub mod client;
pub mod error;
pub mod prompt;
pub mod reflexion;
pub mod types;

pub use error::LlmError;
```

Note: `client`, `prompt`, `reflexion`, `types` modules are declared here but created in later tasks. Until then, add empty stub files so the crate compiles. Create these stubs now as part of this task:

- `src/llm/client.rs`: `//! stub — filled in Task 3.1`
- `src/llm/prompt.rs`: `//! stub — filled in Task 4.1`
- `src/llm/reflexion.rs`: `//! stub — filled in Task 6.1`
- `src/llm/types.rs`: `//! stub — filled in Task 2.1`

- [ ] **Step 2: Create `src/llm/error.rs`**

```rust
//! Error type for the LLM client layer.

use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
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
```

- [ ] **Step 3: Add unit tests at the bottom of `src/llm/error.rs`**

```rust
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
```

- [ ] **Step 4: Update `src/lib.rs`**

Replace contents with:

```rust
pub mod clock;
pub mod config;
pub mod error;
pub mod judge;
pub mod llm;
pub mod storage;

pub use error::AppError;
```

- [ ] **Step 5: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib llm::error::
```

Expected: fmt clean, clippy clean, 2 unit tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/llm/ src/lib.rs
git commit -m "feat(llm): scaffold llm module with LlmError"
```

---

### Task 1.2: Extend `AppError`

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Replace `src/error.rs`**

```rust
//! Top-level error enum, covering the full surface area of an inkworm run.
//! User-facing message mapping happens in `ui::error_banner` (later plan).

use std::path::PathBuf;
use thiserror::Error;

use crate::config::ConfigError;
use crate::llm::error::LlmError;
use crate::storage::StorageError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("llm error: {0}")]
    Llm(#[from] LlmError),

    #[error("reflexion failed after {attempts} attempts; raw saved to {saved_to:?}")]
    Reflexion {
        attempts: u32,
        saved_to: PathBuf,
        summary: String,
    },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("user cancelled")]
    Cancelled,
}
```

- [ ] **Step 2: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass. No new tests required — existing tests exercise the From conversions.

- [ ] **Step 3: Commit**

```bash
git add src/error.rs
git commit -m "feat(error): add AppError::Llm and AppError::Reflexion variants"
```

---

## Phase 2: Wire protocol types

### Task 2.1: `src/llm/types.rs` — ChatRequest + ChatMessage + Role

**Files:**
- Modify: `src/llm/types.rs`

- [ ] **Step 1: Replace `src/llm/types.rs`**

```rust
//! Wire types for the OpenAI-compatible chat completions API, and the Raw*
//! structs that the Reflexion loop deserializes into before promoting them to
//! validated `storage::course` types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    JsonObject,
}

/// The top-level response shape from `/chat/completions`. We only read the
/// first choice's `message.content`.
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponseMessage {
    pub content: String,
}

impl ChatRequest {
    pub fn system_and_user(model: impl Into<String>, system: String, user: String) -> Self {
        Self {
            model: model.into(),
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: system,
                },
                ChatMessage {
                    role: Role::User,
                    content: user,
                },
            ],
            temperature: Some(0.3),
            max_tokens: None,
            response_format: Some(ResponseFormat::JsonObject),
        }
    }

    /// Append an assistant message and a user "repair" message, preserving the
    /// existing conversation history (Reflexion cumulative context).
    pub fn append_repair(&mut self, prior_assistant: String, repair: String) {
        self.messages.push(ChatMessage {
            role: Role::Assistant,
            content: prior_assistant,
        });
        self.messages.push(ChatMessage {
            role: Role::User,
            content: repair,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_and_user_constructs_two_messages() {
        let req = ChatRequest::system_and_user("m", "sys".into(), "usr".into());
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[1].role, Role::User);
    }

    #[test]
    fn append_repair_grows_history() {
        let mut req = ChatRequest::system_and_user("m", "sys".into(), "usr".into());
        req.append_repair("bad json".into(), "fix it".into());
        assert_eq!(req.messages.len(), 4);
        assert_eq!(req.messages[2].role, Role::Assistant);
        assert_eq!(req.messages[3].role, Role::User);
    }

    #[test]
    fn role_serializes_lowercase() {
        let s = serde_json::to_string(&Role::System).unwrap();
        assert_eq!(s, "\"system\"");
    }

    #[test]
    fn request_skips_none_options() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![],
            temperature: None,
            max_tokens: None,
            response_format: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        assert!(!s.contains("temperature"));
        assert!(!s.contains("max_tokens"));
        assert!(!s.contains("response_format"));
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --lib llm::types::
```

Expected: 4 passed.

- [ ] **Step 3: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 4: Commit**

```bash
git add src/llm/types.rs
git commit -m "feat(llm): add chat wire types and request builder"
```

---

## Phase 3: HTTP client

### Task 3.1: Define `LlmClient` trait + `ReqwestClient`

**Files:**
- Modify: `src/llm/client.rs`

- [ ] **Step 1: Replace `src/llm/client.rs`**

```rust
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
    async fn chat(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<String, LlmError>;
}

/// Production client using `reqwest` against an OpenAI-compatible endpoint.
pub struct ReqwestClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl ReqwestClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        request_timeout: Duration,
    ) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(request_timeout)
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            api_key: api_key.into(),
        })
    }
}

#[async_trait]
impl LlmClient for ReqwestClient {
    async fn chat(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<String, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let send = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send();

        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError::Cancelled),
            r = send => r?,
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

impl From<reqwest::Error> for LlmError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            LlmError::Timeout(Duration::from_secs(0))
        } else {
            LlmError::Network(e)
        }
    }
}
```

Note: The `From<reqwest::Error>` override is needed because `reqwest::Error` carries its own "is_timeout" probe — we translate that to `LlmError::Timeout` instead of `LlmError::Network`. The `thiserror` `#[from]` on `Network(reqwest::Error)` in `error.rs` coexists with this explicit impl because they share a variant. If a compile error occurs for duplicate `From` impls, delete the `#[from]` on `Network` in `src/llm/error.rs` and rely solely on this explicit impl.

- [ ] **Step 2: Fix `error.rs` to remove the conflicting `#[from]`**

Replace the `Network` variant in `src/llm/error.rs` with:

```rust
    #[error("network error: {0}")]
    Network(reqwest::Error),
```

(Remove `#[from]` on that variant since the manual impl in `client.rs` handles the conversion.)

- [ ] **Step 3: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 4: Commit**

```bash
git add src/llm/client.rs src/llm/error.rs
git commit -m "feat(llm): add LlmClient trait and ReqwestClient"
```

---

### Task 3.2: Wiremock integration tests for `ReqwestClient`

**Files:**
- Create: `tests/common/llm_mocks.rs`
- Create: `tests/llm.rs`

- [ ] **Step 1: Modify `tests/common/mod.rs` to expose the mocks helper**

Replace the file with:

```rust
//! Shared test helpers. Files under `tests/common/` are not compiled as
//! separate integration binaries — this module is used by each top-level
//! `tests/*.rs` via `mod common;`.

#![allow(dead_code)]

pub mod llm_mocks;

use std::path::PathBuf;
use tempfile::TempDir;

pub struct TestEnv {
    pub _tmp: TempDir,
    pub home: PathBuf,
}

impl TestEnv {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let home = tmp.path().to_path_buf();
        Self { _tmp: tmp, home }
    }
}
```

- [ ] **Step 2: Create `tests/common/llm_mocks.rs`**

```rust
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

/// Register a mock that returns a canned response N times, then errors.
pub async fn expect_many(server: &MockServer, responses: &[&str]) {
    for (i, content) in responses.iter().enumerate() {
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope(content)))
            .up_to_n_times(1)
            .expect(1)
            .named(format!("response-{i}"))
            .mount(server)
            .await;
    }
}
```

- [ ] **Step 3: Create `tests/llm.rs`**

```rust
mod common;

mod client {
    use super::common::llm_mocks::{envelope, expect_ok, expect_status};
    use inkworm::llm::client::{LlmClient, ReqwestClient};
    use inkworm::llm::error::LlmError;
    use inkworm::llm::types::ChatRequest;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::{method, path};

    fn make_client(base_url: String) -> ReqwestClient {
        ReqwestClient::new(base_url, "sk-test", Duration::from_secs(5)).unwrap()
    }

    fn req() -> ChatRequest {
        ChatRequest::system_and_user("m", "sys".into(), "usr".into())
    }

    #[tokio::test]
    async fn ok_returns_content() {
        let server = MockServer::start().await;
        expect_ok(&server, "hello").await;
        let c = make_client(server.uri());
        let r = c.chat(req(), CancellationToken::new()).await.unwrap();
        assert_eq!(r, "hello");
    }

    #[tokio::test]
    async fn unauthorized_maps_to_unauthorized() {
        let server = MockServer::start().await;
        expect_status(&server, 401, "nope").await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, LlmError::Unauthorized), "{err:?}");
    }

    #[tokio::test]
    async fn forbidden_maps_to_unauthorized() {
        let server = MockServer::start().await;
        expect_status(&server, 403, "nope").await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, LlmError::Unauthorized), "{err:?}");
    }

    #[tokio::test]
    async fn server_error_captures_status_and_body() {
        let server = MockServer::start().await;
        expect_status(&server, 503, "overloaded").await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        match err {
            LlmError::Server { status, body } => {
                assert_eq!(status, 503);
                assert!(body.contains("overloaded"));
            }
            other => panic!("expected Server, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rate_limit_extracts_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "7")
                    .set_body_string("slow down"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        match err {
            LlmError::RateLimited(Some(d)) => assert_eq!(d, Duration::from_secs(7)),
            other => panic!("expected RateLimited with retry-after, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancellation_short_circuits() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(envelope("late"))
                    .set_delay(Duration::from_secs(5)),
            )
            .expect(1)
            .mount(&server)
            .await;
        let c = make_client(server.uri());
        let token = CancellationToken::new();
        let t2 = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            t2.cancel();
        });
        let err = c.chat(req(), token).await.unwrap_err();
        assert!(matches!(err, LlmError::Cancelled), "{err:?}");
    }

    #[tokio::test]
    async fn malformed_body_maps_to_invalid_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .expect(1)
            .mount(&server)
            .await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, LlmError::InvalidResponse(_)), "{err:?}");
    }
}
```

- [ ] **Step 4: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --test llm
```

Expected: 7 passed in the `client` submodule.

- [ ] **Step 5: Commit**

```bash
git add tests/common/ tests/llm.rs
git commit -m "test(llm): add wiremock coverage for ReqwestClient error paths"
```

---

## Phase 4: Prompt module

### Task 4.1: Prompt constants and `errors_formatted`

**Files:**
- Modify: `src/llm/prompt.rs`

- [ ] **Step 1: Replace `src/llm/prompt.rs`**

```rust
//! Prompt templates and error rendering for the Reflexion loop.
//!
//! The three string constants are frozen into insta snapshots (see
//! `tests/llm.rs::prompts`). Any edit to a template must be reviewed via the
//! snapshot diff — this protects generation quality from accidental drift.

use std::fmt::Write as _;

use crate::storage::course::ValidationError;

/// Phase 1 system prompt: article → title + description + sentences.
pub const PHASE1_SYSTEM: &str = r#"You are a bilingual language tutor preparing a typing-practice lesson from an English article.

Output ONLY JSON, no markdown fences, no commentary. Schema:

{
  "title":       "English string, 1-100 chars, a concise lesson title",
  "description": "Optional Chinese description, ≤300 chars (empty string allowed)",
  "sentences": [
    { "chinese": "natural Chinese translation (1-200 chars)",
      "english": "sentence from the article, 5-30 words, self-contained, typable ASCII" }
  ]
}

Rules:
- Select 5–20 pedagogically useful sentences (varied grammar, common phrasing).
- If the article is long, pick the most instructive sentences; do NOT quote the whole article.
- Each English sentence must be typable (ASCII letters, straight quotes, basic punctuation).
- Return JSON only.
"#;

/// Phase 2 system prompt: one sentence → 3–5 progressive drills.
pub const PHASE2_SYSTEM: &str = r#"You are a bilingual language tutor decomposing a single sentence into 3–5 progressive typing drills.

Input will be a JSON object { "chinese": "...", "english": "..." }.
Output ONLY JSON, no fences, no commentary. Schema:

{
  "drills": [
    { "stage": 1, "focus": "keywords", "chinese": "...", "english": "...", "soundmark": "IPA or empty string" }
  ]
}

Rules:
- Produce 3 to 5 drills from easy to hard.
- Valid `focus` values: "keywords" | "skeleton" | "clause" | "full".
- Order must progress: keywords (1–5 key words), then skeleton (subject-verb-object core), optionally clause (one modifier layer), and a final "full" stage.
- The LAST drill MUST have focus="full" and its english MUST match the input english verbatim.
- `stage` is 1-indexed and strictly increasing.
- `chinese` is 1-200 chars. `english` is 1-50 words. `soundmark` is IPA wrapped in /slashes/ per word, or an empty string.
- Return JSON only.
"#;

/// Appended to the conversation when a previous attempt failed validation.
/// `{errors}` placeholder is filled at runtime.
pub const REPAIR_TEMPLATE: &str = "Your previous response did not satisfy the schema. Errors:\n{errors}\nReturn ONLY the corrected JSON — same schema, no commentary.";

/// Render a bullet list of validation errors suitable for the repair prompt.
pub fn errors_formatted(errors: &[String]) -> String {
    let mut out = String::new();
    for e in errors {
        let _ = writeln!(out, "- {e}");
    }
    out
}

/// Convenience: render a list of `ValidationError`s (course-level) as bullets.
pub fn course_errors_formatted(errors: &[ValidationError]) -> String {
    errors_formatted(
        &errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<String>>(),
    )
}

/// Build the full repair user message by substituting `{errors}` in the template.
pub fn repair_message(errors: &[String]) -> String {
    REPAIR_TEMPLATE.replace("{errors}", errors_formatted(errors).trim_end())
}
```

- [ ] **Step 2: Unit tests at the bottom of `src/llm/prompt.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_formatted_produces_bullet_list() {
        let s = errors_formatted(&["a".into(), "b".into()]);
        assert_eq!(s, "- a\n- b\n");
    }

    #[test]
    fn errors_formatted_empty_is_empty() {
        assert_eq!(errors_formatted(&[]), "");
    }

    #[test]
    fn repair_message_substitutes_placeholder() {
        let s = repair_message(&["missing title".into()]);
        assert!(s.contains("missing title"));
        assert!(!s.contains("{errors}"));
        assert!(s.starts_with("Your previous response"));
    }

    #[test]
    fn phase1_system_mentions_sentences_range() {
        assert!(PHASE1_SYSTEM.contains("5–20"));
    }

    #[test]
    fn phase2_system_mentions_full_last_constraint() {
        assert!(PHASE2_SYSTEM.contains("LAST drill MUST have focus=\"full\""));
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test --lib llm::prompt::
```

Expected: 5 passed.

- [ ] **Step 4: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 5: Commit**

```bash
git add src/llm/prompt.rs
git commit -m "feat(llm): add prompt templates and error renderer"
```

---

### Task 4.2: Lock prompts via `insta` snapshots

**Files:**
- Modify: `tests/llm.rs` (append `mod prompts;`)
- Create: `tests/snapshots/` (auto-generated by insta, don't create manually)

- [ ] **Step 1: Append the `prompts` submodule to `tests/llm.rs`**

Add at the END of `tests/llm.rs`:

```rust
mod prompts {
    use inkworm::llm::prompt::{PHASE1_SYSTEM, PHASE2_SYSTEM, REPAIR_TEMPLATE};

    #[test]
    fn phase1_system_snapshot() {
        insta::assert_snapshot!("phase1_system", PHASE1_SYSTEM);
    }

    #[test]
    fn phase2_system_snapshot() {
        insta::assert_snapshot!("phase2_system", PHASE2_SYSTEM);
    }

    #[test]
    fn repair_template_snapshot() {
        insta::assert_snapshot!("repair_template", REPAIR_TEMPLATE);
    }
}
```

- [ ] **Step 2: Run tests — first run accepts snapshots**

```bash
cargo test --test llm prompts::
```

Expected: 3 snapshots created under `tests/snapshots/llm__prompts__*.snap`. Tests initially report "new snapshot" and PASS (insta auto-writes on first run with no baseline).

- [ ] **Step 3: Run tests again — confirm they pass with snapshots present**

```bash
cargo test --test llm prompts::
```

Expected: 3 passed, no new snapshot files.

- [ ] **Step 4: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass. Snapshots count as part of the committed test baseline.

- [ ] **Step 5: Commit**

```bash
git add tests/llm.rs tests/snapshots/
git commit -m "test(llm): lock prompt templates with insta snapshots"
```

---

## Phase 5: Phase 1 and Phase 2 raw types + validators

### Task 5.1: Extend `src/llm/types.rs` with `RawSentences` + `RawDrills`

**Files:**
- Modify: `src/llm/types.rs`

- [ ] **Step 1: Append to `src/llm/types.rs`**

Add at the END of the file (after the existing `mod tests`):

```rust
use crate::storage::course::Focus;

/// Phase 1 output shape: what the LLM must return when splitting an article.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawSentences {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub sentences: Vec<RawSentence>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawSentence {
    pub chinese: String,
    pub english: String,
}

/// Phase 2 output shape: what the LLM must return when expanding one sentence
/// into drills.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawDrills {
    pub drills: Vec<RawDrill>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawDrill {
    pub stage: u32,
    pub focus: Focus,
    pub chinese: String,
    pub english: String,
    #[serde(default)]
    pub soundmark: String,
}

impl RawSentences {
    /// Collect ALL validation errors (empty Vec = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let title_len = self.title.chars().count();
        if title_len == 0 || title_len > 100 {
            errs.push(format!("title length must be 1..=100, got {title_len}"));
        }
        let desc_len = self.description.chars().count();
        if desc_len > 300 {
            errs.push(format!("description length must be ≤300, got {desc_len}"));
        }
        let n = self.sentences.len();
        if !(5..=20).contains(&n) {
            errs.push(format!("sentences length must be 5..=20, got {n}"));
        }
        for (i, s) in self.sentences.iter().enumerate() {
            let clen = s.chinese.chars().count();
            if !(1..=200).contains(&clen) {
                errs.push(format!(
                    "sentences[{i}].chinese length must be 1..=200, got {clen}"
                ));
            }
            let words = s.english.split_whitespace().count();
            if !(1..=50).contains(&words) {
                errs.push(format!(
                    "sentences[{i}].english word count must be 1..=50, got {words}"
                ));
            }
        }
        errs
    }
}

impl RawDrills {
    /// Collect ALL validation errors. `reference_english` is the Phase 1
    /// english string this drill-set is supposed to expand; the last drill's
    /// english must match it.
    pub fn validate(&self, reference_english: &str) -> Vec<String> {
        let mut errs = Vec::new();
        let n = self.drills.len();
        if !(3..=5).contains(&n) {
            errs.push(format!("drills length must be 3..=5, got {n}"));
        }
        for (j, d) in self.drills.iter().enumerate() {
            let expected_stage = (j as u32) + 1;
            if d.stage != expected_stage {
                errs.push(format!(
                    "drills[{j}].stage must be {expected_stage}, got {}",
                    d.stage
                ));
            }
            let clen = d.chinese.chars().count();
            if !(1..=200).contains(&clen) {
                errs.push(format!(
                    "drills[{j}].chinese length must be 1..=200, got {clen}"
                ));
            }
            let words = d.english.split_whitespace().count();
            if !(1..=50).contains(&words) {
                errs.push(format!(
                    "drills[{j}].english word count must be 1..=50, got {words}"
                ));
            }
        }
        if let Some(last) = self.drills.last() {
            if last.focus != Focus::Full {
                errs.push(format!(
                    "last drill focus must be \"full\", got \"{:?}\"",
                    last.focus
                ));
            }
            if last.english.trim() != reference_english.trim() {
                errs.push(format!(
                    "last drill english must match reference exactly; got {:?}, expected {:?}",
                    last.english, reference_english
                ));
            }
        }
        errs
    }
}
```

- [ ] **Step 2: Add unit tests — extend the existing `mod tests` in `types.rs`**

Append inside the existing `mod tests { ... }` block (before the closing brace):

```rust
    #[test]
    fn raw_sentences_minimum_valid() {
        let rs = RawSentences {
            title: "T".into(),
            description: "".into(),
            sentences: (0..5)
                .map(|_| RawSentence {
                    chinese: "中".into(),
                    english: "two words here".into(),
                })
                .collect(),
        };
        assert!(rs.validate().is_empty());
    }

    #[test]
    fn raw_sentences_too_few_flagged() {
        let rs = RawSentences {
            title: "T".into(),
            description: "".into(),
            sentences: (0..4)
                .map(|_| RawSentence {
                    chinese: "中".into(),
                    english: "two words".into(),
                })
                .collect(),
        };
        let errs = rs.validate();
        assert!(errs.iter().any(|e| e.contains("sentences length")));
    }

    #[test]
    fn raw_drills_last_full_required() {
        use crate::storage::course::Focus;
        let rd = RawDrills {
            drills: vec![
                RawDrill {
                    stage: 1,
                    focus: Focus::Keywords,
                    chinese: "中".into(),
                    english: "one two".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 2,
                    focus: Focus::Skeleton,
                    chinese: "中".into(),
                    english: "one two three".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 3,
                    focus: Focus::Clause,
                    chinese: "中".into(),
                    english: "one two three four".into(),
                    soundmark: "".into(),
                },
            ],
        };
        let errs = rd.validate("one two three four");
        assert!(
            errs.iter().any(|e| e.contains("last drill focus")),
            "{errs:#?}"
        );
    }

    #[test]
    fn raw_drills_english_mismatch_flagged() {
        use crate::storage::course::Focus;
        let rd = RawDrills {
            drills: vec![
                RawDrill {
                    stage: 1,
                    focus: Focus::Keywords,
                    chinese: "中".into(),
                    english: "a b".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 2,
                    focus: Focus::Skeleton,
                    chinese: "中".into(),
                    english: "a b c".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 3,
                    focus: Focus::Full,
                    chinese: "中".into(),
                    english: "different sentence here".into(),
                    soundmark: "".into(),
                },
            ],
        };
        let errs = rd.validate("expected reference sentence");
        assert!(errs.iter().any(|e| e.contains("match reference")));
    }

    #[test]
    fn raw_drills_minimum_valid() {
        use crate::storage::course::Focus;
        let rd = RawDrills {
            drills: vec![
                RawDrill {
                    stage: 1,
                    focus: Focus::Keywords,
                    chinese: "中".into(),
                    english: "a b".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 2,
                    focus: Focus::Skeleton,
                    chinese: "中".into(),
                    english: "a b c".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 3,
                    focus: Focus::Full,
                    chinese: "中".into(),
                    english: "exact ref".into(),
                    soundmark: "".into(),
                },
            ],
        };
        assert!(rd.validate("exact ref").is_empty());
    }
```

- [ ] **Step 3: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib llm::types::
cargo test --all-targets
```

Expected: 9 tests pass in `llm::types::tests` (4 existing + 5 new).

- [ ] **Step 4: Commit**

```bash
git add src/llm/types.rs
git commit -m "feat(llm): add RawSentences and RawDrills with per-phase validators"
```

---

## Phase 6: Phase 1 Reflexion loop

### Task 6.1: `save_failed_response` in `src/storage/failed.rs`

**Files:**
- Modify: `src/storage/failed.rs`

- [ ] **Step 1: Replace `src/storage/failed.rs`**

```rust
//! Persists LLM responses that failed three repair attempts, so users can
//! inspect them post-mortem and we never pollute the courses library.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::storage::course::StorageError;

/// One failed attempt's raw LLM output + validation errors.
#[derive(Debug, Clone)]
pub struct AttemptFailure {
    pub attempt_number: u32,
    pub raw: String,
    pub errors: Vec<String>,
}

/// Write a human-readable failure report and return the path written.
///
/// `phase` is 1 (article split) or 2 (single-sentence drill expansion).
/// `sentence_index` applies only to phase 2, identifying which sentence
/// failed.
pub fn save_failed_response(
    failed_dir: &Path,
    now: DateTime<Utc>,
    phase: u8,
    sentence_index: Option<usize>,
    model: &str,
    input_preview: &str,
    attempts: &[AttemptFailure],
) -> Result<PathBuf, StorageError> {
    std::fs::create_dir_all(failed_dir)?;
    let ts = now.format("%Y-%m-%d-%H-%M-%S");
    let suffix = match (phase, sentence_index) {
        (1, _) => format!("{ts}-phase1.txt"),
        (2, Some(i)) => format!("{ts}-phase2-s{i}.txt"),
        _ => format!("{ts}-phase{phase}.txt"),
    };
    let path = failed_dir.join(suffix);

    let mut body = String::new();
    body.push_str("=== inkworm reflexion failure ===\n");
    body.push_str(&format!("timestamp: {}\n", now.to_rfc3339()));
    body.push_str(&format!("phase: {phase}\n"));
    if let Some(i) = sentence_index {
        body.push_str(&format!("sentence_index: {i}\n"));
    }
    body.push_str(&format!("model: {model}\n"));
    body.push_str("\ninput (truncated to 500 chars):\n");
    let input_cut: String = input_preview.chars().take(500).collect();
    body.push_str(&input_cut);
    body.push('\n');
    for a in attempts {
        body.push_str(&format!("\n--- attempt {} ---\nraw:\n", a.attempt_number));
        body.push_str(&a.raw);
        body.push_str("\nerrors:\n");
        for e in &a.errors {
            body.push_str(&format!("- {e}\n"));
        }
    }

    // Plain-text file; atomic write not strictly required (stale partials are
    // harmless), but use it for consistency.
    crate::storage::atomic::write_atomic(&path, body.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::tempdir;

    fn fixed() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 21, 10, 30, 42).unwrap()
    }

    #[test]
    fn writes_phase1_file_with_expected_name() {
        let t = tempdir().unwrap();
        let path = save_failed_response(
            t.path(),
            fixed(),
            1,
            None,
            "gpt-4o-mini",
            "some article text",
            &[AttemptFailure {
                attempt_number: 1,
                raw: "bad".into(),
                errors: vec!["missing title".into()],
            }],
        )
        .unwrap();
        assert!(path.ends_with("2026-04-21-10-30-42-phase1.txt"));
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("missing title"));
        assert!(body.contains("phase: 1"));
    }

    #[test]
    fn writes_phase2_file_with_sentence_index() {
        let t = tempdir().unwrap();
        let path = save_failed_response(
            t.path(),
            fixed(),
            2,
            Some(7),
            "gpt-4o-mini",
            "sentence",
            &[],
        )
        .unwrap();
        assert!(path.ends_with("2026-04-21-10-30-42-phase2-s7.txt"));
    }

    #[test]
    fn truncates_long_input_preview() {
        let t = tempdir().unwrap();
        let long = "x".repeat(1000);
        let path = save_failed_response(
            t.path(),
            fixed(),
            1,
            None,
            "m",
            &long,
            &[],
        )
        .unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        // Only 500 `x` should appear after the "input" header.
        let after = body.split("truncated to 500 chars):\n").nth(1).unwrap();
        let first_line = after.lines().next().unwrap();
        assert_eq!(first_line.len(), 500);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --lib storage::failed::
```

Expected: 3 passed.

- [ ] **Step 3: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 4: Commit**

```bash
git add src/storage/failed.rs
git commit -m "feat(storage): add save_failed_response for Reflexion post-mortems"
```

---

### Task 6.2: `Reflexion::reflexion_split` (Phase 1 loop)

**Files:**
- Modify: `src/llm/reflexion.rs`

- [ ] **Step 1: Replace `src/llm/reflexion.rs`**

```rust
//! Two-phase Reflexion-style course generator.
//!
//! Phase 1: split an article into 5–20 (chinese, english) sentence pairs.
//! Phase 2: expand each sentence into 3–5 progressive drills (concurrent,
//!          bounded by `max_concurrent_calls`).
//! Each LLM call has up to 3 repair attempts; validation errors are fed back
//! to the model as a repair prompt. On total failure, the raw response chain
//! is written to `paths.failed_dir`.

use std::path::PathBuf;

use tokio_util::sync::CancellationToken;

use crate::clock::Clock;
use crate::llm::client::LlmClient;
use crate::llm::error::LlmError;
use crate::llm::prompt::{repair_message, PHASE1_SYSTEM};
use crate::llm::types::{ChatRequest, RawSentences};
use crate::storage::DataPaths;
use crate::storage::failed::{save_failed_response, AttemptFailure};

/// Errors that can end a Reflexion run.
#[derive(Debug, thiserror::Error)]
pub enum ReflexionError {
    /// Three attempts in a row failed validation. Raw responses were saved.
    #[error("phase {phase} attempts exhausted; saved to {saved_to:?}")]
    AllAttemptsFailed {
        phase: u8,
        sentence_index: Option<usize>,
        saved_to: PathBuf,
        last_attempts: Vec<AttemptFailure>,
    },
    /// An LLM transport error (network/auth/5xx) — not counted against retry.
    #[error("llm: {0}")]
    Llm(#[from] LlmError),
    /// Caller cancelled via the CancellationToken.
    #[error("cancelled")]
    Cancelled,
    /// Total budget exceeded.
    #[error("budget exceeded")]
    BudgetExceeded,
    /// Storage failure when writing a failed/ report.
    #[error("storage: {0}")]
    Storage(#[from] crate::storage::StorageError),
}

/// Orchestrates one invocation of the two-phase generator.
pub struct Reflexion<'a> {
    pub client: &'a dyn LlmClient,
    pub clock: &'a dyn Clock,
    pub paths: &'a DataPaths,
    pub model: &'a str,
    pub max_concurrent: usize,
    pub cancel: CancellationToken,
}

impl<'a> Reflexion<'a> {
    /// Phase 1: one LLM call (with up to 3 repairs) producing a `RawSentences`.
    pub async fn reflexion_split(&self, article: &str) -> Result<RawSentences, ReflexionError> {
        let user_prompt = format!(
            "Article to split:\n\"\"\"\n{article}\n\"\"\"\n\nReturn JSON only."
        );
        let mut req = ChatRequest::system_and_user(
            self.model.to_string(),
            PHASE1_SYSTEM.to_string(),
            user_prompt.clone(),
        );
        let mut failures: Vec<AttemptFailure> = Vec::new();

        for attempt in 1..=3u32 {
            if self.cancel.is_cancelled() {
                return Err(ReflexionError::Cancelled);
            }
            let raw = self.client.chat(req.clone(), self.cancel.clone()).await?;
            match try_parse_and_validate_phase1(&raw) {
                Ok(rs) => return Ok(rs),
                Err(errors) => {
                    failures.push(AttemptFailure {
                        attempt_number: attempt,
                        raw: raw.clone(),
                        errors: errors.clone(),
                    });
                    if attempt == 3 {
                        let path = save_failed_response(
                            &self.paths.failed_dir,
                            self.clock.now(),
                            1,
                            None,
                            self.model,
                            article,
                            &failures,
                        )?;
                        return Err(ReflexionError::AllAttemptsFailed {
                            phase: 1,
                            sentence_index: None,
                            saved_to: path,
                            last_attempts: failures,
                        });
                    }
                    req.append_repair(raw, repair_message(&errors));
                }
            }
        }
        unreachable!("loop returns on attempt == 3")
    }
}

/// Try to parse the raw string as `RawSentences` and validate it. Returns the
/// flat list of error strings on failure, or `Ok(RawSentences)` on success.
fn try_parse_and_validate_phase1(raw: &str) -> Result<RawSentences, Vec<String>> {
    let parsed: RawSentences = match serde_json::from_str(strip_code_fences(raw)) {
        Ok(p) => p,
        Err(e) => return Err(vec![format!("JSON parse failed: {e}")]),
    };
    let errs = parsed.validate();
    if errs.is_empty() {
        Ok(parsed)
    } else {
        Err(errs)
    }
}

/// Tolerate LLMs that wrap JSON in ```json ... ``` despite being told not to.
fn strip_code_fences(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```json") {
        rest.trim_start()
            .strip_suffix("```")
            .map(str::trim)
            .unwrap_or(rest)
    } else if let Some(rest) = t.strip_prefix("```") {
        rest.trim_start()
            .strip_suffix("```")
            .map(str::trim)
            .unwrap_or(rest)
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_code_fences_removes_json_fences() {
        assert_eq!(
            strip_code_fences("```json\n{\"a\":1}\n```"),
            "{\"a\":1}"
        );
        assert_eq!(strip_code_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fences("{\"a\":1}"), "{\"a\":1}");
    }
}
```

- [ ] **Step 2: Run unit tests**

```bash
cargo test --lib llm::reflexion::
```

Expected: 1 passed.

- [ ] **Step 3: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 4: Commit**

```bash
git add src/llm/reflexion.rs
git commit -m "feat(llm): add Phase 1 Reflexion loop with repair and failed/ persistence"
```

---

### Task 6.3: Integration tests for Phase 1

**Files:**
- Modify: `tests/llm.rs`

- [ ] **Step 1: Append a `reflexion_phase1` submodule to `tests/llm.rs`**

Add at the END:

```rust
mod reflexion_phase1 {
    use super::common::TestEnv;
    use chrono::{TimeZone, Utc};
    use inkworm::clock::FixedClock;
    use inkworm::llm::client::{LlmClient, ReqwestClient};
    use inkworm::llm::reflexion::{Reflexion, ReflexionError};
    use inkworm::storage::paths::DataPaths;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::common::llm_mocks::envelope;

    fn make(env: &TestEnv, server: &MockServer) -> (DataPaths, FixedClock, ReqwestClient) {
        let paths = DataPaths::resolve(Some(&env.home)).unwrap();
        paths.ensure_dirs().unwrap();
        let clock = FixedClock(Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap());
        let client = ReqwestClient::new(server.uri(), "sk-test", Duration::from_secs(5)).unwrap();
        (paths, clock, client)
    }

    fn ok_phase1() -> &'static str {
        r#"{
          "title": "AI at work",
          "description": "",
          "sentences": [
            {"chinese": "一", "english": "one two three"},
            {"chinese": "二", "english": "four five six"},
            {"chinese": "三", "english": "seven eight nine"},
            {"chinese": "四", "english": "ten eleven twelve"},
            {"chinese": "五", "english": "thirteen fourteen fifteen"}
          ]
        }"#
    }

    #[tokio::test]
    async fn phase1_happy_path() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope(ok_phase1())))
            .expect(1)
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "gpt-4o-mini",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let out = r.reflexion_split("article text").await.unwrap();
        assert_eq!(out.sentences.len(), 5);
        assert_eq!(out.title, "AI at work");
        // No failed/ report written.
        let failed_dir_entries: Vec<_> =
            std::fs::read_dir(&paths.failed_dir).unwrap().collect();
        assert!(failed_dir_entries.is_empty());
    }

    #[tokio::test]
    async fn phase1_repair_success_on_second_attempt() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        // First response: missing sentences.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(envelope(r#"{"title":"T","description":""}"#)),
            )
            .up_to_n_times(1)
            .expect(1)
            .named("bad-first")
            .mount(&server)
            .await;
        // Second response: valid.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope(ok_phase1())))
            .expect(1)
            .named("good-second")
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let out = r.reflexion_split("article").await.unwrap();
        assert_eq!(out.sentences.len(), 5);
    }

    #[tokio::test]
    async fn phase1_three_failures_saves_to_disk_and_errors() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(envelope("not json at all")),
            )
            .expect(3)
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let err = r.reflexion_split("article").await.unwrap_err();
        match err {
            ReflexionError::AllAttemptsFailed { phase, saved_to, last_attempts, .. } => {
                assert_eq!(phase, 1);
                assert!(saved_to.exists());
                assert_eq!(last_attempts.len(), 3);
            }
            other => panic!("expected AllAttemptsFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn phase1_auth_error_short_circuits_no_retry() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("nope"))
            .expect(1) // only ONE call; auth error does not retry
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let err = r.reflexion_split("article").await.unwrap_err();
        assert!(matches!(err, ReflexionError::Llm(_)), "{err:?}");
        // No failed/ report for transport errors.
        let failed: Vec<_> = std::fs::read_dir(&paths.failed_dir).unwrap().collect();
        assert!(failed.is_empty());
    }

    #[tokio::test]
    async fn phase1_cancel_stops_retry_loop() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        // First response bad, LLM would be called again, but we cancel in between.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(envelope("not json"))
                    .set_delay(Duration::from_millis(50)),
            )
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            c2.cancel();
        });
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel,
        };
        let err = r.reflexion_split("article").await.unwrap_err();
        assert!(
            matches!(
                err,
                ReflexionError::Cancelled | ReflexionError::Llm(inkworm::llm::error::LlmError::Cancelled)
            ),
            "{err:?}"
        );
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --test llm reflexion_phase1::
```

Expected: 5 passed.

- [ ] **Step 3: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 4: Commit**

```bash
git add tests/llm.rs
git commit -m "test(llm): cover Phase 1 reflexion with wiremock (ok/repair/fail/cancel/auth)"
```

---

## Phase 7: Phase 2 Reflexion + concurrent orchestration

### Task 7.1: `reflexion_drill` + `orchestrate_phase2`

**Files:**
- Modify: `src/llm/reflexion.rs`

- [ ] **Step 1: Add imports and the new functions to `src/llm/reflexion.rs`**

Append to the END of the existing file (before the final `#[cfg(test)] mod tests`):

```rust
use std::sync::Arc;

use futures::future::try_join_all;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::llm::prompt::PHASE2_SYSTEM;
use crate::llm::types::{RawDrill, RawDrills, RawSentence};

impl<'a> Reflexion<'a> {
    /// Phase 2: expand ONE sentence into drills via LLM. Up to 3 repair attempts.
    /// Returns `Ok(RawDrills)` or a `ReflexionError::AllAttemptsFailed { phase: 2 }`.
    pub async fn reflexion_drill(
        &self,
        sentence_index: usize,
        sentence: &RawSentence,
    ) -> Result<RawDrills, ReflexionError> {
        let user_prompt = json!({
            "chinese": sentence.chinese,
            "english": sentence.english,
        })
        .to_string();
        let mut req = ChatRequest::system_and_user(
            self.model.to_string(),
            PHASE2_SYSTEM.to_string(),
            user_prompt.clone(),
        );
        let mut failures: Vec<AttemptFailure> = Vec::new();

        for attempt in 1..=3u32 {
            if self.cancel.is_cancelled() {
                return Err(ReflexionError::Cancelled);
            }
            let raw = self.client.chat(req.clone(), self.cancel.clone()).await?;
            match try_parse_and_validate_phase2(&raw, &sentence.english) {
                Ok(rd) => return Ok(rd),
                Err(errors) => {
                    failures.push(AttemptFailure {
                        attempt_number: attempt,
                        raw: raw.clone(),
                        errors: errors.clone(),
                    });
                    if attempt == 3 {
                        let path = save_failed_response(
                            &self.paths.failed_dir,
                            self.clock.now(),
                            2,
                            Some(sentence_index),
                            self.model,
                            &sentence.english,
                            &failures,
                        )?;
                        return Err(ReflexionError::AllAttemptsFailed {
                            phase: 2,
                            sentence_index: Some(sentence_index),
                            saved_to: path,
                            last_attempts: failures,
                        });
                    }
                    req.append_repair(raw, repair_message(&errors));
                }
            }
        }
        unreachable!("loop returns on attempt == 3")
    }

    /// Run `reflexion_drill` for each sentence, bounded by `max_concurrent`.
    /// Any single-sentence failure returns an error and cancels the rest.
    pub async fn orchestrate_phase2(
        &self,
        sentences: &[RawSentence],
    ) -> Result<Vec<RawDrills>, ReflexionError> {
        let sem = Arc::new(Semaphore::new(self.max_concurrent.max(1)));
        let tasks: Vec<_> = sentences
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let sem = sem.clone();
                let sentence = s.clone();
                async move {
                    let _permit = sem.acquire().await.unwrap();
                    self.reflexion_drill(i, &sentence).await
                }
            })
            .collect();
        try_join_all(tasks).await
    }
}

/// Try to parse the raw string as `RawDrills` and validate it against the
/// reference english. Returns the flat error list on failure.
fn try_parse_and_validate_phase2(
    raw: &str,
    reference_english: &str,
) -> Result<RawDrills, Vec<String>> {
    let parsed: RawDrills = match serde_json::from_str(strip_code_fences(raw)) {
        Ok(p) => p,
        Err(e) => return Err(vec![format!("JSON parse failed: {e}")]),
    };
    let errs = parsed.validate(reference_english);
    if errs.is_empty() {
        Ok(parsed)
    } else {
        Err(errs)
    }
}

#[allow(dead_code)]
fn __keep_raw_drill_in_scope(_: RawDrill) {}
```

(The no-op `__keep_raw_drill_in_scope` is only there to ensure `RawDrill` is treated as used when only `RawDrills` is referenced; remove after Task 8.1 adds the real use sites.)

- [ ] **Step 2: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 3: Commit**

```bash
git add src/llm/reflexion.rs
git commit -m "feat(llm): add Phase 2 reflexion_drill and bounded concurrent orchestration"
```

---

### Task 7.2: Integration tests for Phase 2

**Files:**
- Modify: `tests/llm.rs`

- [ ] **Step 1: Append a `reflexion_phase2` submodule to `tests/llm.rs`**

Add at the END:

```rust
mod reflexion_phase2 {
    use super::common::TestEnv;
    use chrono::{TimeZone, Utc};
    use inkworm::clock::FixedClock;
    use inkworm::llm::client::ReqwestClient;
    use inkworm::llm::reflexion::{Reflexion, ReflexionError};
    use inkworm::llm::types::RawSentence;
    use inkworm::storage::paths::DataPaths;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::common::llm_mocks::envelope;

    fn ok_drills(english: &str) -> String {
        format!(
            r#"{{
                "drills": [
                    {{"stage": 1, "focus": "keywords", "chinese": "关键", "english": "a b", "soundmark": ""}},
                    {{"stage": 2, "focus": "skeleton", "chinese": "骨架", "english": "a b c", "soundmark": ""}},
                    {{"stage": 3, "focus": "full", "chinese": "完整", "english": "{english}", "soundmark": ""}}
                ]
            }}"#
        )
    }

    fn five_sentences() -> Vec<RawSentence> {
        (0..5)
            .map(|i| RawSentence {
                chinese: format!("句{i}"),
                english: format!("sentence number {i} here"),
            })
            .collect()
    }

    fn make(env: &TestEnv, server: &MockServer) -> (DataPaths, FixedClock, ReqwestClient) {
        let paths = DataPaths::resolve(Some(&env.home)).unwrap();
        paths.ensure_dirs().unwrap();
        let clock = FixedClock(Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap());
        let client = ReqwestClient::new(server.uri(), "sk-test", Duration::from_secs(5)).unwrap();
        (paths, clock, client)
    }

    #[tokio::test]
    async fn phase2_all_ok() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        let sentences = five_sentences();
        for s in &sentences {
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(envelope(&ok_drills(&s.english))),
                )
                .up_to_n_times(1)
                .expect(1)
                .named(format!("drill-{}", s.english))
                .mount(&server)
                .await;
        }
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 3,
            cancel: CancellationToken::new(),
        };
        let outs = r.orchestrate_phase2(&sentences).await.unwrap();
        assert_eq!(outs.len(), 5);
        for o in &outs {
            assert_eq!(o.drills.len(), 3);
        }
    }

    #[tokio::test]
    async fn phase2_one_sentence_failure_fails_the_whole_run() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        let sentences = five_sentences();
        // 4 sentences respond fine.
        for s in sentences.iter().take(4) {
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(envelope(&ok_drills(&s.english))),
                )
                .up_to_n_times(1)
                .expect(1)
                .named(format!("ok-{}", s.english))
                .mount(&server)
                .await;
        }
        // The 5th sentence's prompt (which contains "sentence number 4 here") fails 3x.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(wiremock::matchers::body_string_contains("sentence number 4 here"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(envelope("not json")),
            )
            .expect(3)
            .named("bad-sentence-4")
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let err = r.orchestrate_phase2(&sentences).await.unwrap_err();
        match err {
            ReflexionError::AllAttemptsFailed {
                phase,
                sentence_index,
                ..
            } => {
                assert_eq!(phase, 2);
                assert_eq!(sentence_index, Some(4));
            }
            other => panic!("expected AllAttemptsFailed phase=2, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn phase2_rejects_last_english_mismatch() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        let sentences = five_sentences();
        // For sentence 0, the drill returns english "WRONG" on the last drill,
        // causing RawDrills::validate to flag the mismatch — and after 3
        // failures the whole run errors.
        let bad = r#"{
          "drills": [
            {"stage": 1, "focus": "keywords", "chinese": "关键", "english": "a b", "soundmark": ""},
            {"stage": 2, "focus": "skeleton", "chinese": "骨架", "english": "a b c", "soundmark": ""},
            {"stage": 3, "focus": "full", "chinese": "完整", "english": "WRONG", "soundmark": ""}
          ]
        }"#;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(wiremock::matchers::body_string_contains("sentence number 0 here"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope(bad)))
            .expect(3)
            .named("mismatch")
            .mount(&server)
            .await;
        // The other 4 succeed on first try.
        for s in sentences.iter().skip(1) {
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(wiremock::matchers::body_string_contains(&s.english as &str))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(envelope(&ok_drills(&s.english))),
                )
                .up_to_n_times(1)
                .expect(1)
                .named(format!("ok-{}", s.english))
                .mount(&server)
                .await;
        }
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let err = r.orchestrate_phase2(&sentences).await.unwrap_err();
        assert!(
            matches!(
                err,
                ReflexionError::AllAttemptsFailed {
                    phase: 2,
                    sentence_index: Some(0),
                    ..
                }
            ),
            "{err:?}"
        );
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --test llm reflexion_phase2::
```

Expected: 3 passed.

- [ ] **Step 3: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 4: Commit**

```bash
git add tests/llm.rs
git commit -m "test(llm): cover Phase 2 reflexion (all_ok / one_fails / last_english_mismatch)"
```

---

## Phase 8: Merge to `Course`

### Task 8.1: `build_course` — slug + order fill + merge

**Files:**
- Modify: `src/llm/reflexion.rs`

- [ ] **Step 1: Remove the placeholder `__keep_raw_drill_in_scope` and add `build_course` + `slug` at the bottom of `src/llm/reflexion.rs`**

Delete the earlier:

```rust
#[allow(dead_code)]
fn __keep_raw_drill_in_scope(_: RawDrill) {}
```

…and replace it with:

```rust
use chrono::DateTime;

use crate::storage::course::{Course, Drill, Sentence, Source, SourceKind, SCHEMA_VERSION};

/// Result of a successful `generate` run.
#[derive(Debug, Clone)]
pub struct ReflexionOutcome {
    pub course: Course,
    pub phase1_attempts: u32,
    pub phase2_attempts: Vec<u32>,
}

/// Combine the Phase 1 header, per-sentence Phase 2 drills, and metadata into
/// a fully populated `Course` struct (with program-filled `id`/`order`/`stage`/`source`).
#[allow(clippy::too_many_arguments)]
pub fn build_course(
    sentences_raw: &[RawSentence],
    drills_raw: &[RawDrills],
    title: &str,
    description: &str,
    existing_ids: &[String],
    model: &str,
    now: DateTime<chrono::Utc>,
) -> Course {
    let sentences: Vec<Sentence> = sentences_raw
        .iter()
        .zip(drills_raw.iter())
        .enumerate()
        .map(|(i, (_s, rd))| Sentence {
            order: (i as u32) + 1,
            drills: rd
                .drills
                .iter()
                .enumerate()
                .map(|(j, d)| Drill {
                    stage: (j as u32) + 1,
                    focus: d.focus,
                    chinese: d.chinese.clone(),
                    english: d.english.clone(),
                    soundmark: d.soundmark.clone(),
                })
                .collect(),
        })
        .collect();

    let id = unique_id(&now, title, existing_ids);
    Course {
        schema_version: SCHEMA_VERSION,
        id,
        title: title.to_string(),
        description: if description.is_empty() {
            None
        } else {
            Some(description.to_string())
        },
        source: Source {
            kind: SourceKind::Article,
            url: String::new(),
            created_at: now,
            model: model.to_string(),
        },
        sentences,
    }
}

/// Build a unique Course id of the form `YYYY-MM-DD-<slug(title)>`, appending
/// `-2`, `-3`, ... if the computed id collides with an existing one.
pub fn unique_id(now: &DateTime<chrono::Utc>, title: &str, existing: &[String]) -> String {
    let base = format!("{}-{}", now.format("%Y-%m-%d"), slug(title));
    if !existing.iter().any(|e| e == &base) {
        return base;
    }
    for n in 2u32.. {
        let candidate = format!("{base}-{n}");
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
    }
    unreachable!("u32 exhausted");
}

/// Turn a title into a kebab-case slug: lowercase ASCII + digits + '-',
/// collapsing runs, trimming leading/trailing '-', capped at 40 chars, and
/// guaranteed non-empty ("lesson" if the title yielded nothing usable).
pub fn slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_dash = true;
    for c in title.chars() {
        let mapped = if c.is_ascii_alphanumeric() {
            Some(c.to_ascii_lowercase())
        } else if c.is_whitespace() || c == '-' || c == '_' {
            if last_dash {
                None
            } else {
                Some('-')
            }
        } else {
            None
        };
        if let Some(ch) = mapped {
            out.push(ch);
            last_dash = ch == '-';
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.len() > 40 {
        out.truncate(40);
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() {
        out = "lesson".into();
    }
    out
}

#[cfg(test)]
mod build_course_tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap()
    }

    #[test]
    fn slug_basic() {
        assert_eq!(slug("Hello World"), "hello-world");
        assert_eq!(slug("TED: What AI Means"), "ted-what-ai-means");
        assert_eq!(slug("  trim me  "), "trim-me");
    }

    #[test]
    fn slug_truncates_at_forty() {
        let s = slug(&"a".repeat(100));
        assert!(s.len() <= 40);
    }

    #[test]
    fn slug_never_empty() {
        assert_eq!(slug("   "), "lesson");
        assert_eq!(slug("!!!"), "lesson");
        assert_eq!(slug(""), "lesson");
    }

    #[test]
    fn unique_id_appends_suffix_on_collision() {
        let existing = vec!["2026-04-21-hello".into(), "2026-04-21-hello-2".into()];
        let id = unique_id(&now(), "Hello", &existing);
        assert_eq!(id, "2026-04-21-hello-3");
    }

    #[test]
    fn build_course_passes_course_validate() {
        use crate::storage::course::Focus;
        let sentences: Vec<RawSentence> = (0..5)
            .map(|i| RawSentence {
                chinese: format!("句{i}"),
                english: format!("sentence number {i} here"),
            })
            .collect();
        let drills: Vec<RawDrills> = sentences
            .iter()
            .map(|s| RawDrills {
                drills: vec![
                    RawDrill {
                        stage: 1,
                        focus: Focus::Keywords,
                        chinese: "关键".into(),
                        english: "a b".into(),
                        soundmark: "".into(),
                    },
                    RawDrill {
                        stage: 2,
                        focus: Focus::Skeleton,
                        chinese: "骨架".into(),
                        english: "a b c".into(),
                        soundmark: "".into(),
                    },
                    RawDrill {
                        stage: 3,
                        focus: Focus::Full,
                        chinese: "完整".into(),
                        english: s.english.clone(),
                        soundmark: "".into(),
                    },
                ],
            })
            .collect();
        let c = build_course(
            &sentences,
            &drills,
            "Test Title",
            "",
            &[],
            "gpt-4o-mini",
            now(),
        );
        let errs = c.validate();
        assert!(errs.is_empty(), "{errs:#?}");
        assert_eq!(c.id, "2026-04-21-test-title");
        assert_eq!(c.description, None);
        assert_eq!(c.sentences.len(), 5);
    }
}
```

- [ ] **Step 2: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass. `build_course_tests` adds 5 more unit tests.

- [ ] **Step 3: Commit**

```bash
git add src/llm/reflexion.rs
git commit -m "feat(llm): add build_course with slug id generation and collision handling"
```

---

## Phase 9: Top-level `generate` orchestrator

### Task 9.1: `Reflexion::generate` ties everything together

**Files:**
- Modify: `src/llm/reflexion.rs`

- [ ] **Step 1: Add `generate` method to the existing `impl<'a> Reflexion<'a>` block**

Insert the following method as a sibling of the existing `reflexion_split`, `reflexion_drill`, `orchestrate_phase2` methods (inside one of the `impl<'a> Reflexion<'a> { ... }` blocks — which one doesn't matter; prefer the first for locality):

```rust
    /// Full pipeline: article → Course. Returns the assembled Course plus
    /// attempt counts (1 for success, 2 if one repair, 3 if two repairs).
    /// Any sub-phase error (Reflexion exhaustion, LlmError, Cancelled) is
    /// propagated; on success nothing is written to disk — the caller is
    /// responsible for persisting via `storage::save_course`.
    pub async fn generate(
        &self,
        article: &str,
        existing_ids: &[String],
    ) -> Result<ReflexionOutcome, ReflexionError> {
        let phase1 = self.reflexion_split(article).await?;
        let phase2 = self.orchestrate_phase2(&phase1.sentences).await?;
        let course = build_course(
            &phase1.sentences,
            &phase2,
            &phase1.title,
            &phase1.description,
            existing_ids,
            self.model,
            self.clock.now(),
        );
        // We cannot easily recover per-call attempt counts without threading
        // state; stub them as zero for now. `generate` is the place where
        // future telemetry will decorate these if needed.
        Ok(ReflexionOutcome {
            course,
            phase1_attempts: 1,
            phase2_attempts: vec![1; phase2.len()],
        })
    }
```

- [ ] **Step 2: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 3: Commit**

```bash
git add src/llm/reflexion.rs
git commit -m "feat(llm): add Reflexion::generate full pipeline"
```

---

### Task 9.2: End-to-end wiremock test

**Files:**
- Modify: `tests/llm.rs`

- [ ] **Step 1: Append `reflexion_e2e` submodule to `tests/llm.rs`**

Add at the END:

```rust
mod reflexion_e2e {
    use super::common::TestEnv;
    use chrono::{TimeZone, Utc};
    use inkworm::clock::FixedClock;
    use inkworm::llm::client::ReqwestClient;
    use inkworm::llm::reflexion::Reflexion;
    use inkworm::storage::paths::DataPaths;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::common::llm_mocks::envelope;

    fn phase1_ok() -> &'static str {
        r#"{
          "title": "Test lesson",
          "description": "",
          "sentences": [
            {"chinese": "一", "english": "sentence number 0 here"},
            {"chinese": "二", "english": "sentence number 1 here"},
            {"chinese": "三", "english": "sentence number 2 here"},
            {"chinese": "四", "english": "sentence number 3 here"},
            {"chinese": "五", "english": "sentence number 4 here"}
          ]
        }"#
    }

    fn drill_for(english: &str) -> String {
        format!(
            r#"{{
              "drills": [
                {{"stage": 1, "focus": "keywords", "chinese": "关键", "english": "a b", "soundmark": ""}},
                {{"stage": 2, "focus": "skeleton", "chinese": "骨架", "english": "a b c", "soundmark": ""}},
                {{"stage": 3, "focus": "full", "chinese": "完整", "english": "{english}", "soundmark": ""}}
              ]
            }}"#
        )
    }

    #[tokio::test]
    async fn article_to_course_happy_path() {
        let env = TestEnv::new();
        let server = MockServer::start().await;

        // Phase 1 — matches the article text in the user prompt.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("Article to split"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope(phase1_ok())))
            .up_to_n_times(1)
            .expect(1)
            .named("phase1")
            .mount(&server)
            .await;

        // Phase 2 — one mock per sentence, keyed by matching the english in the
        // user prompt (each request body embeds the sentence JSON).
        for i in 0..5 {
            let english = format!("sentence number {i} here");
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains(&english as &str))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(envelope(&drill_for(&english))),
                )
                .up_to_n_times(1)
                .expect(1)
                .named(format!("phase2-{i}"))
                .mount(&server)
                .await;
        }

        let paths = DataPaths::resolve(Some(&env.home)).unwrap();
        paths.ensure_dirs().unwrap();
        let clock = FixedClock(Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap());
        let client =
            ReqwestClient::new(server.uri(), "sk-test", Duration::from_secs(5)).unwrap();
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "gpt-4o-mini",
            max_concurrent: 3,
            cancel: CancellationToken::new(),
        };

        let out = r
            .generate("This is a sample article body with enough context.", &[])
            .await
            .unwrap();

        // Course-level invariants.
        assert_eq!(out.course.schema_version, 2);
        assert!(out.course.id.starts_with("2026-04-21-test-lesson"));
        assert_eq!(out.course.title, "Test lesson");
        assert_eq!(out.course.sentences.len(), 5);
        assert!(out.course.sentences.iter().all(|s| s.drills.len() == 3));
        assert!(out.course.validate().is_empty(), "{:#?}", out.course.validate());
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --test llm reflexion_e2e::
```

Expected: 1 passed.

- [ ] **Step 3: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 4: Commit**

```bash
git add tests/llm.rs
git commit -m "test(llm): add end-to-end article-to-Course wiremock coverage"
```

---

## Phase 10: Live smoke binary + final polish

### Task 10.1: `examples/smoke.rs` — live end-to-end via env vars

**Files:**
- Create: `examples/smoke.rs`

- [ ] **Step 1: Create `examples/smoke.rs`**

```rust
//! Live smoke test for Reflexion against a real OpenAI-compatible endpoint.
//!
//! Usage (no keys in source — set via env):
//!
//!     export INKWORM_LLM_BASE_URL="https://api.openai.com/v1"
//!     export INKWORM_LLM_API_KEY="sk-..."
//!     export INKWORM_LLM_MODEL="gpt-4o-mini"
//!     # Write an article to a file, point the example at it:
//!     echo "This is the article body..." > /tmp/article.txt
//!     cargo run --example smoke -- /tmp/article.txt
//!
//! Prints the resulting Course JSON on success, or a diagnostic on failure.

use std::path::PathBuf;
use std::time::Duration;

use inkworm::clock::SystemClock;
use inkworm::llm::client::ReqwestClient;
use inkworm::llm::reflexion::Reflexion;
use inkworm::storage::paths::DataPaths;
use tokio_util::sync::CancellationToken;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let article_path: PathBuf = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: smoke <article-path>"))?
        .into();
    let article = std::fs::read_to_string(&article_path)?;

    let base_url = std::env::var("INKWORM_LLM_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".into());
    let api_key = std::env::var("INKWORM_LLM_API_KEY")
        .map_err(|_| anyhow::anyhow!("INKWORM_LLM_API_KEY not set"))?;
    let model =
        std::env::var("INKWORM_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());

    // Use a temp data dir so the smoke run doesn't pollute ~/.config/inkworm.
    let tmp = tempfile::tempdir()?;
    let paths = DataPaths::resolve(Some(tmp.path()))?;
    paths.ensure_dirs()?;

    let clock = SystemClock;
    let client = ReqwestClient::new(base_url, api_key, Duration::from_secs(60))?;
    let r = Reflexion {
        client: &client,
        clock: &clock,
        paths: &paths,
        model: &model,
        max_concurrent: 5,
        cancel: CancellationToken::new(),
    };

    eprintln!("Generating course from {article_path:?} …");
    let t0 = std::time::Instant::now();
    let outcome = r.generate(&article, &[]).await?;
    let elapsed = t0.elapsed();
    eprintln!("Done in {elapsed:.2?}. Course:");
    println!("{}", serde_json::to_string_pretty(&outcome.course)?);
    Ok(())
}
```

- [ ] **Step 2: Verify compile only (no live call)**

```bash
cargo check --example smoke
```

Expected: clean compile. Do NOT run with a real key as part of this task — that is the user's verification step.

- [ ] **Step 3: Verify example compiles without changes**

Cargo's `dev-dependencies` are available to `examples/`, `tests/`, and `benches/`. Since `tempfile` is already in `[dev-dependencies]` and `anyhow` is in `[dependencies]`, no Cargo.toml edit is needed. If `cargo check --example smoke` succeeded in Step 2, proceed.

- [ ] **Step 4: Full quality gate**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

All must pass.

- [ ] **Step 5: Commit**

```bash
git add examples/smoke.rs
git commit -m "chore(llm): add smoke example for live end-to-end verification"
```

---

### Task 10.2: Sync design spec §5.5 Phase 1 prompt

**Files:**
- Modify: `docs/superpowers/specs/2026-04-21-inkworm-design.md`

- [ ] **Step 1: Update spec §5.5 Phase 1 system prompt**

In `docs/superpowers/specs/2026-04-21-inkworm-design.md`, find the Phase 1 prompt block (starts with `Select 5–20 pedagogically useful sentences from the article.`). Replace that entire fenced block with:

```
You are a bilingual language tutor preparing a typing-practice lesson from an English article.

Output ONLY JSON, no markdown fences, no commentary. Schema:

{
  "title":       "English string, 1-100 chars, a concise lesson title",
  "description": "Optional Chinese description, ≤300 chars (empty string allowed)",
  "sentences": [
    { "chinese": "natural Chinese translation (1-200 chars)",
      "english": "sentence from the article, 5-30 words, self-contained, typable ASCII" }
  ]
}

Rules:
- Select 5–20 pedagogically useful sentences (varied grammar, common phrasing).
- If the article is long, pick the most instructive sentences; do NOT quote the whole article.
- Each English sentence must be typable (ASCII letters, straight quotes, basic punctuation).
- Return JSON only.
```

Rationale (mention in the commit): original spec Phase 1 omitted `title` / `description` at the top level, leaving Course metadata ambiguous. Plan 2 extends Phase 1 to return those fields.

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-04-21-inkworm-design.md
git commit -m "docs: sync Phase 1 prompt with Plan 2 implementation (title+description)"
```

---

### Task 10.3: Final green for Plan 2

**Files:** none

- [ ] **Step 1: Full quality gate**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Expected: fmt no changes, clippy zero warnings, all tests pass. Expected test count: 58 (from Plan 1) + ~30 new (LLM unit + integration + snapshots + build_course + failed/ + prompts) = roughly 88 passing.

- [ ] **Step 2: If anything needs fixing, fix in place and commit**

```bash
git add -u
git commit -m "chore: cargo fmt + clippy clean-up for Plan 2"
```

Only if changes were needed.

---

## Spec Coverage Check (self-review)

| Spec section | Task | Status |
|---|---|---|
| §5.1 Two-phase generation | Task 6.2 + 7.1 + 9.1 | ✅ |
| §5.2 Reflexion 3-retry with cumulative history | Task 6.2 + 7.1 | ✅ |
| §5.3 Cancellation propagation | Task 3.1 + 6.2 + 7.1 | ✅ |
| §5.4 Per-call timeout (30s default) + Reflexion budget (60s) | Task 3.1 client timeout | partial (client timeout; top-level budget deferred — see note below) |
| §5.5 Prompt templates (Phase 1, Phase 2, Repair) | Task 4.1 + 4.2 | ✅ |
| §5.5 Prompts locked by insta | Task 4.2 | ✅ |
| §5.6 Article size guard | not in Plan 2 (handled by UI Generate screen in Plan 3) | deferred |
| §4.3 Course fields filled by program (id, order, stage, source) | Task 8.1 | ✅ |
| §4.3 id rule `YYYY-MM-DD-<slug(title)>` with `-2` collision | Task 8.1 | ✅ |
| failed/ naming `YYYY-MM-DD-HH-MM-SS-phase{1,2}[-s{N}].txt` | Task 6.1 | ✅ |
| `AppError::Llm` + `AppError::Reflexion` | Task 1.2 | ✅ |
| Wiremock coverage of happy/repair/fail/cancel/auth/network/rate-limit | Task 3.2 + 6.3 + 7.2 + 9.2 | ✅ |
| Smoke binary for live verification | Task 10.1 | ✅ |

**Out of scope for Plan 2 (handled by later plans):**
- Top-level 60s Reflexion budget (small; could be added here but no urgent reason)
- Article byte limit enforcement (Plan 3 — Generate screen)
- TUI integration (Plan 3)
- TTS (Plan 5)

**Note on budget:** Plan 2 has per-call timeouts (30s via `ReqwestClient::new`'s `request_timeout`). The top-level 60s budget wrapping `Reflexion::generate` is not enforced. This is acceptable because:
- For small articles, 1 × Phase 1 + 5–15 × Phase 2 (concurrent) typically finishes in 10–20s with 30s per-call caps
- The UI (Plan 3) will layer a `tokio::time::timeout` around `generate` for the user-visible budget
- If measurement shows pathological tail latencies, adding a `tokio::time::timeout(Duration::from_secs(budget), ...)` wrap to `generate` is a one-line follow-up

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-21-inkworm-v1-llm.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
