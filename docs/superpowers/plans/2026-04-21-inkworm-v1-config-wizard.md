# Config Wizard Implementation Plan (Plan 4b)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add first-run + `/config` wizard that collects LLM endpoint / api_key / model, validates with a 1-token connectivity probe, and atomically saves — so the app stops exiting on first launch.

**Architecture:** New `Screen::ConfigWizard` owning `WizardState` with 3 steps (Endpoint/ApiKey/Model). Model commit spawns a tokio task running a minimal `ReqwestClient::chat` probe; result flows back via existing `task_rx` as `TaskMsg::Wizard(WizardTaskMsg)`. On success: re-read existing config, patch LLM fields only (preserve TTS/data/generation), atomic write. `Config::validate` splits into `validate_llm` + `validate_tts` so main.rs only gates on LLM fields.

**Tech Stack:** ratatui, tokio mpsc, `tokio_util::sync::CancellationToken`, existing `ReqwestClient`/`ChatRequest`/`ChatResponse`, existing `write_atomic`.

**Parent spec:** `docs/superpowers/specs/2026-04-21-inkworm-v1-config-wizard-design.md`

---

## File Structure

```
src/
├── app.rs                       # [MODIFY] Screen::ConfigWizard, field, handlers, save logic
├── config/mod.rs                # [MODIFY] split validate() into validate_llm/_tts
├── ui/
│   ├── mod.rs                   # [MODIFY] pub mod config_wizard
│   ├── config_wizard.rs         # [CREATE] WizardState, step logic, render, probe_llm
│   ├── task_msg.rs              # [MODIFY] add Wizard variant
│   └── palette.rs               # [MODIFY] /config available: true
└── main.rs                      # [MODIFY] tolerate missing/invalid config, open wizard

tests/
└── config_wizard.rs             # [CREATE] integration tests
```

---

## Task 1: Split `Config::validate` into `validate_llm` + `validate_tts`

**Files:**
- Modify: `src/config/mod.rs`

- [ ] **Step 1: Write failing unit tests**

Append to `src/config/mod.rs` (inside existing `#[cfg(test)] mod tests` block — if the file has no tests module yet, create one at the end of the file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_llm_catches_missing_api_key() {
        let cfg = Config::default();
        let errs = cfg.validate_llm();
        assert!(errs.iter().any(|e| matches!(e, ConfigError::MissingField("llm.api_key"))));
    }

    #[test]
    fn validate_llm_does_not_flag_tts_issues() {
        // Default config has tts.enabled=true and empty iflytek fields — but that's TTS's problem, not LLM's.
        let mut cfg = Config::default();
        cfg.llm.api_key = "sk-ok".into();
        let errs = cfg.validate_llm();
        assert!(errs.is_empty(), "got {errs:?}");
    }

    #[test]
    fn validate_tts_flags_missing_iflytek_when_enabled() {
        let cfg = Config::default();
        let errs = cfg.validate_tts();
        assert!(errs.iter().any(|e| matches!(e, ConfigError::MissingField("tts.iflytek.app_id"))));
    }

    #[test]
    fn validate_delegates_to_both() {
        let cfg = Config::default();
        let full = cfg.validate();
        let llm = cfg.validate_llm();
        let tts = cfg.validate_tts();
        assert_eq!(full.len(), llm.len() + tts.len());
    }

    #[test]
    fn validate_llm_flags_zero_concurrency() {
        let mut cfg = Config::default();
        cfg.llm.api_key = "sk-ok".into();
        cfg.generation.max_concurrent_calls = 0;
        let errs = cfg.validate_llm();
        assert!(errs.iter().any(|e| matches!(e, ConfigError::Invalid { field: "generation.max_concurrent_calls", .. })));
    }
}
```

- [ ] **Step 2: Run tests — expect failures**

Run: `cargo test --lib config::tests`
Expected: FAIL (no `validate_llm` / `validate_tts` methods yet)

- [ ] **Step 3: Implement the split**

Replace the existing `validate` method in `src/config/mod.rs` (around line 204) with three methods:

```rust
    /// LLM + generation subsystem fields (gated by main.rs at startup).
    pub fn validate_llm(&self) -> Vec<ConfigError> {
        let mut errs = Vec::new();
        if self.llm.api_key.trim().is_empty() {
            errs.push(ConfigError::MissingField("llm.api_key"));
        }
        if self.llm.base_url.trim().is_empty() {
            errs.push(ConfigError::MissingField("llm.base_url"));
        }
        if self.llm.model.trim().is_empty() {
            errs.push(ConfigError::MissingField("llm.model"));
        }
        if self.generation.max_concurrent_calls == 0 {
            errs.push(ConfigError::Invalid {
                field: "generation.max_concurrent_calls",
                reason: "must be ≥1".into(),
            });
        }
        errs
    }

    /// TTS subsystem fields (checked separately — Plan 6 owns TTS).
    pub fn validate_tts(&self) -> Vec<ConfigError> {
        let mut errs = Vec::new();
        if self.tts.enabled && self.tts.r#override != TtsOverride::Off {
            if self.tts.iflytek.app_id.trim().is_empty() {
                errs.push(ConfigError::MissingField("tts.iflytek.app_id"));
            }
            if self.tts.iflytek.api_key.trim().is_empty() {
                errs.push(ConfigError::MissingField("tts.iflytek.api_key"));
            }
            if self.tts.iflytek.api_secret.trim().is_empty() {
                errs.push(ConfigError::MissingField("tts.iflytek.api_secret"));
            }
        }
        errs
    }

    /// Collects ALL validation errors, does not short-circuit.
    pub fn validate(&self) -> Vec<ConfigError> {
        let mut errs = self.validate_llm();
        errs.extend(self.validate_tts());
        errs
    }
```

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test --lib config`
Expected: all config tests PASS (including pre-existing ones if any)

- [ ] **Step 5: Run full test suite to catch regressions**

Run: `cargo test`
Expected: all tests PASS (behavior of `validate()` unchanged)

- [ ] **Step 6: Commit**

```bash
git add src/config/mod.rs
git commit -m "refactor(config): split validate into validate_llm and validate_tts"
```

---

## Task 2: `WizardState` types + step-transition unit tests

**Files:**
- Create: `src/ui/config_wizard.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create `src/ui/config_wizard.rs` with state types and commit logic**

```rust
//! Config wizard: 3-step first-run / `/config` flow for LLM endpoint / api_key / model.
//! Connectivity probe and rendering live in this file (probe in §Probe block, render in §Render block).

use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::ui::error_banner::UserMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Endpoint,
    ApiKey,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardOrigin {
    FirstRun,
    Command,
}

#[derive(Debug)]
pub struct TestingState {
    pub cancel_token: CancellationToken,
}

#[derive(Debug)]
pub struct WizardState {
    pub step: WizardStep,
    pub origin: WizardOrigin,
    pub draft: Config,
    pub input: String,
    pub testing: Option<TestingState>,
    pub error: Option<UserMessage>,
}

/// Outcome of `WizardState::commit` — tells App what to do next.
#[derive(Debug)]
pub enum CommitOutcome {
    /// Advance to next step (input already seeded with draft value for the new step).
    Advance,
    /// On Model step — spawn connectivity test.
    ProbeConnectivity,
    /// Input was invalid (e.g., empty). `error` is now set; stay on same step.
    Invalid,
}

/// Outcome of `WizardState::back` — tells App what to do next.
#[derive(Debug)]
pub enum BackOutcome {
    /// Moved to previous step (input seeded).
    Back,
    /// Stayed (FirstRun on Endpoint).
    NoOp,
    /// Abort wizard (Command on Endpoint).
    Abort,
}

impl WizardState {
    pub fn new(origin: WizardOrigin, draft: Config) -> Self {
        let input = draft.llm.base_url.clone();
        Self {
            step: WizardStep::Endpoint,
            origin,
            draft,
            input,
            testing: None,
            error: None,
        }
    }

    pub fn type_char(&mut self, c: char) {
        self.error = None;
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        self.error = None;
        self.input.pop();
    }

    /// Commit current step. On Endpoint/ApiKey: Advance; on Model: ProbeConnectivity.
    pub fn commit(&mut self) -> CommitOutcome {
        self.error = None;
        let trimmed = self.input.trim();
        if trimmed.is_empty() {
            let label = match self.step {
                WizardStep::Endpoint => "Endpoint cannot be empty",
                WizardStep::ApiKey => "API key cannot be empty",
                WizardStep::Model => "Model cannot be empty",
            };
            self.error = Some(UserMessage {
                headline: label.to_string(),
                hint: String::new(),
                severity: crate::ui::error_banner::Severity::Error,
            });
            return CommitOutcome::Invalid;
        }
        match self.step {
            WizardStep::Endpoint => {
                self.draft.llm.base_url = trimmed.to_string();
                self.step = WizardStep::ApiKey;
                self.input = self.draft.llm.api_key.clone();
                CommitOutcome::Advance
            }
            WizardStep::ApiKey => {
                // ApiKey stores full input (no trim — some keys start/end with whitespace? no, trim is safe).
                self.draft.llm.api_key = trimmed.to_string();
                self.step = WizardStep::Model;
                self.input = self.draft.llm.model.clone();
                CommitOutcome::Advance
            }
            WizardStep::Model => {
                self.draft.llm.model = trimmed.to_string();
                CommitOutcome::ProbeConnectivity
            }
        }
    }

    /// Go back one step. See BackOutcome for semantics.
    pub fn back(&mut self) -> BackOutcome {
        self.error = None;
        match self.step {
            WizardStep::Endpoint => match self.origin {
                WizardOrigin::FirstRun => BackOutcome::NoOp,
                WizardOrigin::Command => BackOutcome::Abort,
            },
            WizardStep::ApiKey => {
                self.step = WizardStep::Endpoint;
                self.input = self.draft.llm.base_url.clone();
                BackOutcome::Back
            }
            WizardStep::Model => {
                self.step = WizardStep::ApiKey;
                self.input = self.draft.llm.api_key.clone();
                BackOutcome::Back
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_wiz() -> WizardState {
        WizardState::new(WizardOrigin::FirstRun, Config::default())
    }

    #[test]
    fn new_starts_on_endpoint_with_default_url() {
        let w = new_wiz();
        assert_eq!(w.step, WizardStep::Endpoint);
        assert_eq!(w.input, "https://api.openai.com/v1");
    }

    #[test]
    fn commit_endpoint_advances_to_apikey() {
        let mut w = new_wiz();
        w.input = "https://example.com/v1".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::Advance));
        assert_eq!(w.step, WizardStep::ApiKey);
        assert_eq!(w.draft.llm.base_url, "https://example.com/v1");
    }

    #[test]
    fn commit_empty_sets_error_and_stays() {
        let mut w = new_wiz();
        w.input = "   ".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::Invalid));
        assert_eq!(w.step, WizardStep::Endpoint);
        assert!(w.error.is_some());
    }

    #[test]
    fn commit_model_requests_probe() {
        let mut w = new_wiz();
        w.input = "https://example.com/v1".into();
        w.commit();
        w.input = "sk-123".into();
        w.commit();
        assert_eq!(w.step, WizardStep::Model);
        w.input = "gpt-4o-mini".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::ProbeConnectivity));
        assert_eq!(w.draft.llm.model, "gpt-4o-mini");
    }

    #[test]
    fn back_from_apikey_returns_to_endpoint_with_seeded_input() {
        let mut w = new_wiz();
        w.input = "https://x/v1".into();
        w.commit();
        w.input = "sk-typed".into();
        let outcome = w.back();
        assert!(matches!(outcome, BackOutcome::Back));
        assert_eq!(w.step, WizardStep::Endpoint);
        assert_eq!(w.input, "https://x/v1");
    }

    #[test]
    fn back_on_endpoint_firstrun_is_noop() {
        let mut w = WizardState::new(WizardOrigin::FirstRun, Config::default());
        let outcome = w.back();
        assert!(matches!(outcome, BackOutcome::NoOp));
        assert_eq!(w.step, WizardStep::Endpoint);
    }

    #[test]
    fn back_on_endpoint_command_aborts() {
        let mut w = WizardState::new(WizardOrigin::Command, Config::default());
        let outcome = w.back();
        assert!(matches!(outcome, BackOutcome::Abort));
    }

    #[test]
    fn type_and_backspace_clear_error() {
        let mut w = new_wiz();
        w.input.clear();
        w.commit();
        assert!(w.error.is_some());
        w.type_char('a');
        assert!(w.error.is_none());
        w.commit();
        assert!(w.error.is_some());
        w.backspace();
        assert!(w.error.is_none());
    }
}
```

- [ ] **Step 2: Register module in `src/ui/mod.rs`**

Add a new line in the pub mod list (alphabetical placement between `config_wizard` and the rest):

```rust
pub mod config_wizard;
pub mod error_banner;
pub mod event;
pub mod generate;
pub mod palette;
pub mod skeleton;
pub mod study;
pub mod task_msg;
pub mod terminal;
```

(Insert only the `pub mod config_wizard;` line at the top; don't rewrite the whole list if it already exists.)

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::config_wizard`
Expected: 8 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/ui/config_wizard.rs src/ui/mod.rs
git commit -m "feat(ui): add WizardState with step-transition logic"
```

---

## Task 3: `TaskMsg::Wizard` variant + `probe_llm` helper

**Files:**
- Modify: `src/ui/task_msg.rs`
- Modify: `src/ui/config_wizard.rs`

- [ ] **Step 1: Add `WizardTaskMsg` + extend `TaskMsg`**

Replace `src/ui/task_msg.rs` contents with:

```rust
use crate::error::AppError;
use crate::storage::course::Course;

/// Messages sent from background tasks to the main event loop.
#[derive(Debug)]
pub enum TaskMsg {
    Generate(GenerateProgress),
    Wizard(WizardTaskMsg),
}

/// Progress updates from the Generate background task.
#[derive(Debug)]
pub enum GenerateProgress {
    Phase1Started,
    Phase1Done { sentence_count: usize },
    Phase2Progress { done: usize, total: usize },
    Done(Course),
    Failed(AppError),
}

/// Result from the ConfigWizard connectivity probe.
#[derive(Debug)]
pub enum WizardTaskMsg {
    ConnectivityOk,
    ConnectivityFailed(AppError),
}
```

- [ ] **Step 2: Add `probe_llm` to `config_wizard.rs`**

Append to `src/ui/config_wizard.rs` (after the `#[cfg(test)] mod tests { ... }` block — module items must come outside the tests module):

```rust
use std::time::Duration;

use crate::config::LlmConfig;
use crate::error::AppError;
use crate::llm::client::{LlmClient, ReqwestClient};
use crate::llm::types::{ChatMessage, ChatRequest, Role};

/// Fire a minimal 1-token chat request to verify credentials and model work.
/// Maps any LlmError into AppError. Cancellation via the token returns
/// AppError::Cancelled.
pub async fn probe_llm(llm: LlmConfig, cancel: CancellationToken) -> Result<(), AppError> {
    let client = ReqwestClient::new(
        llm.base_url.clone(),
        llm.api_key.clone(),
        Duration::from_secs(llm.request_timeout_secs),
    )
    .map_err(AppError::Llm)?;

    let req = ChatRequest {
        model: llm.model.clone(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: "ping".into(),
        }],
        temperature: Some(0.0),
        max_tokens: Some(1),
        response_format: None,
    };

    match client.chat(req, cancel).await {
        Ok(_content) => Ok(()),
        Err(crate::llm::error::LlmError::Cancelled) => Err(AppError::Cancelled),
        Err(e) => Err(AppError::Llm(e)),
    }
}
```

- [ ] **Step 3: Add probe integration test**

Append to the `#[cfg(test)] mod tests` block in `src/ui/config_wizard.rs` (inside the existing tests module, before the final `}`):

```rust
    #[tokio::test]
    async fn probe_llm_ok_on_200() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role":"assistant","content":"pong"}}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let llm = LlmConfig {
            base_url: server.uri(),
            api_key: "sk-test".into(),
            model: "gpt-4o-mini".into(),
            request_timeout_secs: 5,
            reflexion_budget_secs: 60,
        };
        let res = probe_llm(llm, CancellationToken::new()).await;
        assert!(res.is_ok(), "{res:?}");
    }

    #[tokio::test]
    async fn probe_llm_maps_401_to_llm_unauthorized() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("no"))
            .expect(1)
            .mount(&server)
            .await;

        let llm = LlmConfig {
            base_url: server.uri(),
            api_key: "sk-test".into(),
            model: "gpt-4o-mini".into(),
            request_timeout_secs: 5,
            reflexion_budget_secs: 60,
        };
        let err = probe_llm(llm, CancellationToken::new()).await.unwrap_err();
        assert!(
            matches!(err, AppError::Llm(crate::llm::error::LlmError::Unauthorized)),
            "{err:?}"
        );
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib ui::config_wizard`
Expected: 10 tests PASS (8 existing + 2 new async)

- [ ] **Step 5: Add placeholder arm in `src/app.rs` so `on_task_msg` remains exhaustive**

Edit `src/app.rs` — `on_task_msg` method (around line 95):

```rust
    pub fn on_task_msg(&mut self, msg: TaskMsg) {
        match msg {
            TaskMsg::Generate(progress) => self.handle_generate_progress(progress),
            TaskMsg::Wizard(_) => {} // placeholder — wired up in Task 5
        }
    }
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check`
Expected: compiles with no errors.

- [ ] **Step 7: Commit**

```bash
git add src/ui/task_msg.rs src/ui/config_wizard.rs src/app.rs
git commit -m "feat(ui): add WizardTaskMsg and probe_llm connectivity check"
```

---

## Task 4: `App` integration — `Screen::ConfigWizard`, field, `open_wizard`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add `Screen::ConfigWizard` variant**

Edit `src/app.rs` — the `Screen` enum (around line 19):

```rust
pub enum Screen {
    Study,
    Palette,
    Help,
    Generate,
    DeleteConfirm,
    ConfigWizard,   // NEW
}
```

- [ ] **Step 2: Add `config_wizard` field to `App`**

In the `App` struct (around line 27), add a new field after `delete_confirming`:

```rust
pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
    pub study: StudyState,
    pub palette: Option<PaletteState>,
    pub data_paths: DataPaths,
    pub clock: Arc<dyn Clock>,
    blink_counter: u32,
    pub cursor_visible: bool,
    pub task_tx: mpsc::Sender<TaskMsg>,
    pub generate: Option<GenerateSubstate>,
    pub config: Config,
    pub delete_confirming: Option<String>,
    pub config_wizard: Option<crate::ui::config_wizard::WizardState>,  // NEW
}
```

- [ ] **Step 3: Initialize field in `App::new`**

Update `App::new` return struct literal (around line 50):

```rust
Self {
    screen: Screen::Study,
    should_quit: false,
    study: StudyState::new(course, progress),
    palette: None,
    data_paths,
    clock,
    blink_counter: 0,
    cursor_visible: true,
    task_tx,
    generate: None,
    config,
    delete_confirming: None,
    config_wizard: None,   // NEW
}
```

- [ ] **Step 4: Add `open_wizard` method**

Add after the `App::new` block (and before `on_tick` or at end of `impl App`):

```rust
    pub fn open_wizard(&mut self, origin: crate::ui::config_wizard::WizardOrigin) {
        use crate::ui::config_wizard::WizardState;
        let state = WizardState::new(origin, self.config.clone());
        self.config_wizard = Some(state);
        self.screen = Screen::ConfigWizard;
    }
```

- [ ] **Step 5: Handle `Screen::ConfigWizard` in `on_input` match**

Find `on_input` (around line 75) and add the new arm:

```rust
Event::Key(key) => match &self.screen {
    Screen::Study => self.handle_study_key(key),
    Screen::Palette => self.handle_palette_key(key),
    Screen::Help => self.handle_help_key(key),
    Screen::Generate => self.handle_generate_key(key),
    Screen::DeleteConfirm => self.handle_delete_confirm_key(key),
    Screen::ConfigWizard => self.handle_config_wizard_key(key),   // NEW
},
```

- [ ] **Step 6: Add stub `handle_config_wizard_key` so it compiles**

Add at the end of `impl App` (just before the closing brace):

```rust
    fn handle_config_wizard_key(&mut self, _key: KeyEvent) {
        // Implemented in Task 5
    }
```

- [ ] **Step 7: Handle `Screen::ConfigWizard` in `render`**

Edit the `render` method (around line 461). Add a new arm before the closing brace:

```rust
Screen::ConfigWizard => {
    if let Some(ref state) = self.config_wizard {
        crate::ui::config_wizard::render_config_wizard(frame, state, self.cursor_visible);
    }
}
```

Since `render_config_wizard` isn't defined until Task 6, stub this arm for now:

```rust
Screen::ConfigWizard => {
    // render added in Task 6
}
```

- [ ] **Step 8: Verify compilation**

Run: `cargo check`
Expected: compiles with warnings only (unused `handle_config_wizard_key` OK)

- [ ] **Step 9: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add Screen::ConfigWizard and open_wizard scaffolding"
```

---

## Task 5: Wizard key handling + connectivity spawn + save dispatch

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Implement `handle_config_wizard_key`**

Replace the stub added in Task 4 with:

```rust
    fn handle_config_wizard_key(&mut self, key: KeyEvent) {
        use crate::ui::config_wizard::{BackOutcome, CommitOutcome};

        let is_testing = self
            .config_wizard
            .as_ref()
            .and_then(|s| s.testing.as_ref())
            .is_some();

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit();
            return;
        }

        if is_testing {
            if key.code == KeyCode::Esc {
                if let Some(ref mut state) = self.config_wizard {
                    if let Some(ref t) = state.testing {
                        t.cancel_token.cancel();
                    }
                    state.testing = None;
                }
            }
            return;
        }

        match key.code {
            KeyCode::Esc => {
                let outcome = self
                    .config_wizard
                    .as_mut()
                    .map(|s| s.back())
                    .unwrap_or(BackOutcome::NoOp);
                match outcome {
                    BackOutcome::Back | BackOutcome::NoOp => {}
                    BackOutcome::Abort => {
                        self.config_wizard = None;
                        self.screen = Screen::Study;
                    }
                }
            }
            KeyCode::Enter => {
                let outcome = self
                    .config_wizard
                    .as_mut()
                    .map(|s| s.commit())
                    .unwrap_or(CommitOutcome::Invalid);
                match outcome {
                    CommitOutcome::ProbeConnectivity => {
                        self.spawn_connectivity_test();
                    }
                    CommitOutcome::Advance | CommitOutcome::Invalid => {}
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut state) = self.config_wizard {
                    state.backspace();
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut state) = self.config_wizard {
                    state.type_char(c);
                }
            }
            _ => {}
        }
    }
```

- [ ] **Step 2: Implement `spawn_connectivity_test`**

Add after `handle_config_wizard_key` in `impl App`:

```rust
    fn spawn_connectivity_test(&mut self) {
        use crate::ui::config_wizard::{TestingState, probe_llm};
        use crate::ui::task_msg::WizardTaskMsg;

        let llm = match self.config_wizard.as_ref() {
            Some(s) => s.draft.llm.clone(),
            None => return,
        };
        let cancel = CancellationToken::new();
        if let Some(state) = self.config_wizard.as_mut() {
            state.testing = Some(TestingState {
                cancel_token: cancel.clone(),
            });
        }
        let task_tx = self.task_tx.clone();
        tokio::spawn(async move {
            let msg = match probe_llm(llm, cancel).await {
                Ok(()) => WizardTaskMsg::ConnectivityOk,
                Err(e) => WizardTaskMsg::ConnectivityFailed(e),
            };
            let _ = task_tx.send(TaskMsg::Wizard(msg)).await;
        });
    }
```

- [ ] **Step 3: Add `CancellationToken` import**

Near the top of `src/app.rs` imports, add:

```rust
use tokio_util::sync::CancellationToken;
```

- [ ] **Step 4: Implement `handle_wizard_task_msg` with atomic save**

Add after `spawn_connectivity_test`:

```rust
    fn handle_wizard_task_msg(&mut self, msg: crate::ui::task_msg::WizardTaskMsg) {
        use crate::ui::error_banner::user_message;
        use crate::ui::task_msg::WizardTaskMsg;

        let Some(wizard) = self.config_wizard.as_mut() else { return };
        wizard.testing = None;

        match msg {
            WizardTaskMsg::ConnectivityOk => {
                // Re-read existing config.toml to preserve non-LLM fields (TTS etc.).
                // Fall back to Config::default() if file missing or parse error.
                let mut merged = Config::load(&self.data_paths.config_file).unwrap_or_default();
                merged.llm = wizard.draft.llm.clone();
                match merged.write_atomic(&self.data_paths.config_file) {
                    Ok(()) => {
                        self.config = merged;
                        self.config_wizard = None;
                        self.screen = Screen::Study;
                    }
                    Err(e) => {
                        let app_err = crate::error::AppError::Config(e);
                        wizard.error = Some(user_message(&app_err));
                    }
                }
            }
            WizardTaskMsg::ConnectivityFailed(e) => {
                wizard.error = Some(user_message(&e));
            }
        }
    }
```

- [ ] **Step 5: Dispatch `TaskMsg::Wizard` in `on_task_msg`**

Update `on_task_msg` (around line 95):

```rust
    pub fn on_task_msg(&mut self, msg: TaskMsg) {
        match msg {
            TaskMsg::Generate(progress) => self.handle_generate_progress(progress),
            TaskMsg::Wizard(m) => self.handle_wizard_task_msg(m),
        }
    }
```

Remove the temporary `TaskMsg::Wizard(_) => {}` arm added in Task 3 if still present.

- [ ] **Step 6: Verify compilation**

Run: `cargo check`
Expected: compiles with no warnings

- [ ] **Step 7: Run lib tests (no UI integration yet)**

Run: `cargo test --lib`
Expected: all PASS

- [ ] **Step 8: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): wire ConfigWizard key handling and connectivity probe"
```

---

## Task 6: Wizard rendering

**Files:**
- Modify: `src/ui/config_wizard.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Extract pure helpers and add tests**

Append to `src/ui/config_wizard.rs` — add pure helpers **inside** the existing `impl WizardState` block (or as free functions near the types). Use free functions for clarity:

```rust
/// Title line for the wizard frame.
pub fn wizard_title(step: WizardStep) -> String {
    let n = match step {
        WizardStep::Endpoint => 1,
        WizardStep::ApiKey => 2,
        WizardStep::Model => 3,
    };
    format!("inkworm — setup ({n} / 3)")
}

/// Step-specific label.
pub fn wizard_step_label(step: WizardStep) -> &'static str {
    match step {
        WizardStep::Endpoint => "LLM endpoint",
        WizardStep::ApiKey => "LLM API key",
        WizardStep::Model => "LLM model",
    }
}

/// Display-ready input — masks the ApiKey step, passes through otherwise.
pub fn mask_for_display(input: &str, step: WizardStep) -> String {
    if step == WizardStep::ApiKey {
        "*".repeat(input.chars().count())
    } else {
        input.to_string()
    }
}

/// Hint line at the bottom of the wizard.
pub fn wizard_hint(step: WizardStep, origin: WizardOrigin, testing: bool) -> &'static str {
    if testing {
        return "Testing connectivity…     Esc · cancel";
    }
    match (step, origin) {
        (WizardStep::Endpoint, WizardOrigin::FirstRun) => "Enter · next     Ctrl+C · quit",
        (WizardStep::Endpoint, WizardOrigin::Command) => "Enter · next     Esc · cancel",
        (WizardStep::ApiKey, _) => "Enter · next     Esc · back",
        (WizardStep::Model, _) => "Enter · test and save     Esc · back",
    }
}
```

Then append these unit tests **inside** the existing `#[cfg(test)] mod tests` block (before the final `}` of that module):

```rust
    #[test]
    fn mask_hides_apikey_but_preserves_others() {
        assert_eq!(
            mask_for_display("supersecret", WizardStep::ApiKey),
            "***********"
        );
        assert_eq!(
            mask_for_display("https://x/v1", WizardStep::Endpoint),
            "https://x/v1"
        );
        assert_eq!(
            mask_for_display("gpt-4o-mini", WizardStep::Model),
            "gpt-4o-mini"
        );
    }

    #[test]
    fn mask_counts_unicode_chars_not_bytes() {
        // Each CJK char is multi-byte — we count chars, so mask length = char count.
        assert_eq!(mask_for_display("你好", WizardStep::ApiKey), "**");
    }

    #[test]
    fn title_shows_1_of_3_on_endpoint() {
        assert_eq!(wizard_title(WizardStep::Endpoint), "inkworm — setup (1 / 3)");
        assert_eq!(wizard_title(WizardStep::ApiKey), "inkworm — setup (2 / 3)");
        assert_eq!(wizard_title(WizardStep::Model), "inkworm — setup (3 / 3)");
    }

    #[test]
    fn hint_on_endpoint_firstrun_says_ctrl_c_quit() {
        let h = wizard_hint(WizardStep::Endpoint, WizardOrigin::FirstRun, false);
        assert!(h.contains("Ctrl+C"));
        assert!(!h.contains("back"));
    }

    #[test]
    fn hint_on_endpoint_command_says_esc_cancel() {
        let h = wizard_hint(WizardStep::Endpoint, WizardOrigin::Command, false);
        assert!(h.contains("Esc"));
        assert!(h.contains("cancel"));
    }

    #[test]
    fn hint_during_testing_mentions_connectivity() {
        let h = wizard_hint(WizardStep::Model, WizardOrigin::FirstRun, true);
        assert!(h.to_lowercase().contains("testing"));
    }
```

- [ ] **Step 2: Append render function**

Append the render function at the end of `src/ui/config_wizard.rs`:

```rust
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn render_config_wizard(frame: &mut Frame, state: &WizardState, cursor_visible: bool) {
    let area = frame.area();

    let title = wizard_title(state.step);
    let label = wizard_step_label(state.step);
    let rendered_input = mask_for_display(&state.input, state.step);
    let cursor_glyph = if cursor_visible { "_" } else { " " };
    let hint = wizard_hint(state.step, state.origin, state.testing.is_some());

    let has_error = state.error.is_some();
    let block_height: u16 = if has_error { 8 } else { 6 };
    let top = area.height.saturating_sub(block_height) / 2;
    let left = area.width / 5;
    let width = area.width.saturating_sub(left * 2);

    let title_line = Paragraph::new(Span::styled(
        title,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(title_line, Rect::new(left, top, width, 1));

    let label_line = Paragraph::new(Span::styled(
        label,
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(label_line, Rect::new(left, top + 2, width, 1));

    let input_line = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::DarkGray)),
        Span::styled(rendered_input, Style::default().fg(Color::White)),
        Span::styled(cursor_glyph, Style::default().fg(Color::White)),
    ]));
    frame.render_widget(input_line, Rect::new(left, top + 3, width, 1));

    let hint_line = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)));
    frame.render_widget(hint_line, Rect::new(left, top + 5, width, 1));

    if let Some(ref err) = state.error {
        let color = match err.severity {
            crate::ui::error_banner::Severity::Error => Color::Red,
            crate::ui::error_banner::Severity::Warning => Color::Yellow,
            crate::ui::error_banner::Severity::Info => Color::Blue,
        };
        let err_line = Paragraph::new(Span::styled(
            err.headline.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(err_line, Rect::new(left, top + 7, width, 1));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::config_wizard`
Expected: previous 10 tests + 6 new helper tests = 16 PASS

- [ ] **Step 4: Replace render stub in `app.rs`**

If Task 4 Step 7 left a stub, replace it:

```rust
Screen::ConfigWizard => {
    if let Some(ref state) = self.config_wizard {
        crate::ui::config_wizard::render_config_wizard(frame, state, self.cursor_visible);
    }
}
```

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all PASS

- [ ] **Step 6: Commit**

```bash
git add src/ui/config_wizard.rs src/app.rs
git commit -m "feat(ui): add config wizard rendering with masked api key"
```

---

## Task 7: `/config` palette command + main.rs bootstrap

**Files:**
- Modify: `src/ui/palette.rs`
- Modify: `src/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Enable `/config` in palette**

Edit `src/ui/palette.rs` — the `COMMANDS` array (line 9):

```rust
    Command { name: "config", aliases: &[], description: "Configuration wizard", available: true },
```

- [ ] **Step 2: Handle `"config"` in `execute_command`**

Edit `src/app.rs` — in `execute_command` (around line 437):

```rust
    fn execute_command(&mut self, cmd: &Command) {
        match cmd.name {
            "quit" | "q" => self.quit(),
            "skip" => self.study.skip(),
            "help" => self.screen = Screen::Help,
            "import" => {
                self.generate = Some(GenerateSubstate::Pasting(PastingState::new()));
                self.screen = Screen::Generate;
            }
            "config" => {
                self.open_wizard(crate::ui::config_wizard::WizardOrigin::Command);
            }
            "delete" => {
                if let Some(course) = self.study.current_course() {
                    self.delete_confirming = Some(course.title.clone());
                    self.screen = Screen::DeleteConfirm;
                }
            }
            _ => {}
        }
    }
```

- [ ] **Step 3: Update `main.rs` to tolerate missing/invalid config and open wizard**

Replace the body of `main.rs` with:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use inkworm::app::App;
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::load_course;
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::ui::config_wizard::WizardOrigin;
use inkworm::ui::event::run_loop;
use inkworm::ui::terminal::{install_panic_hook, TerminalGuard};

fn main() -> anyhow::Result<()> {
    install_panic_hook();

    let cli_config: Option<PathBuf> = std::env::args()
        .nth(1)
        .filter(|a| a == "--config")
        .and_then(|_| std::env::args().nth(2))
        .map(PathBuf::from);

    let paths = DataPaths::resolve(cli_config.as_deref())?;
    paths.ensure_dirs()?;

    let (config, needs_wizard) = match Config::load(&paths.config_file) {
        Ok(c) if c.validate_llm().is_empty() => (c, false),
        Ok(c) => (c, true),
        Err(_) => (Config::default(), true),
    };

    let progress = Progress::load(&paths.progress_file)?;

    let course = progress
        .active_course_id
        .as_deref()
        .and_then(|id| load_course(&paths.courses_dir, id).ok());

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let mut guard = TerminalGuard::new()?;
        let (task_tx, task_rx) = tokio::sync::mpsc::channel(32);
        let mut app = App::new(
            course,
            progress,
            paths,
            Arc::new(SystemClock),
            config,
            task_tx,
        );
        if needs_wizard {
            app.open_wizard(WizardOrigin::FirstRun);
        }
        run_loop(&mut guard, &mut app, task_rx).await
    })?;

    Ok(())
}
```

- [ ] **Step 4: Verify compilation + tests**

Run: `cargo check && cargo test`
Expected: all PASS

- [ ] **Step 5: Commit**

```bash
git add src/ui/palette.rs src/app.rs src/main.rs
git commit -m "feat(app): wire /config command and first-run wizard bootstrap"
```

---

## Task 8: Integration tests

**Files:**
- Create: `tests/config_wizard.rs`

- [ ] **Step 1: Create `tests/config_wizard.rs`**

```rust
mod common;

use std::path::PathBuf;
use std::sync::Arc;

use inkworm::app::{App, Screen};
use inkworm::clock::SystemClock;
use inkworm::config::{Config, IflytekConfig};
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::ui::config_wizard::{WizardOrigin, WizardStep};
use inkworm::ui::task_msg::{TaskMsg, WizardTaskMsg};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn setup(tmp: &TempDir) -> (DataPaths, Progress) {
    let paths = DataPaths::resolve(Some(tmp.path())).unwrap();
    paths.ensure_dirs().unwrap();
    let progress = Progress::load(&paths.progress_file).unwrap();
    (paths, progress)
}

fn make_app(paths: DataPaths, progress: Progress, cfg: Config) -> (App, tokio::sync::mpsc::Receiver<TaskMsg>) {
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let app = App::new(
        None,
        progress,
        paths,
        Arc::new(SystemClock),
        cfg,
        tx,
    );
    (app, rx)
}

#[tokio::test]
async fn first_run_opens_wizard() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths, progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    assert!(matches!(app.screen, Screen::ConfigWizard));
    let w = app.config_wizard.as_ref().unwrap();
    assert_eq!(w.step, WizardStep::Endpoint);
    assert_eq!(w.origin, WizardOrigin::FirstRun);
}

#[tokio::test]
async fn connectivity_ok_saves_and_dismisses() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role":"assistant","content":"pong"}}]
        })))
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths.clone(), progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    // Simulate wizard: set fields directly on the draft, drive into Model step.
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.draft.llm.base_url = server.uri();
        w.draft.llm.api_key = "sk-test".into();
        w.draft.llm.model = "gpt-4o-mini".into();
        w.step = WizardStep::Model;
    }

    // Simulate "ConnectivityOk" arriving.
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::ConnectivityOk));

    assert!(matches!(app.screen, Screen::Study));
    assert!(app.config_wizard.is_none());
    // Config reloaded from disk must contain the new LLM fields.
    let saved = Config::load(&paths.config_file).unwrap();
    assert_eq!(saved.llm.base_url, server.uri());
    assert_eq!(saved.llm.api_key, "sk-test");
    assert_eq!(saved.llm.model, "gpt-4o-mini");
}

#[tokio::test]
async fn connectivity_failed_keeps_wizard_open_with_error() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths, progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.step = WizardStep::Model;
    }

    let err = inkworm::error::AppError::Llm(inkworm::llm::error::LlmError::Unauthorized);
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::ConnectivityFailed(err)));

    assert!(matches!(app.screen, Screen::ConfigWizard));
    let w = app.config_wizard.as_ref().unwrap();
    assert!(w.testing.is_none());
    assert!(w.error.is_some(), "error banner should be set");
}

#[tokio::test]
async fn atomic_save_preserves_tts_and_generation_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role":"assistant","content":"pong"}}]
        })))
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);

    // Pre-write a config.toml with TTS fields populated.
    let mut existing = Config::default();
    existing.tts.iflytek = IflytekConfig {
        app_id: "APP123".into(),
        api_key: "KEY456".into(),
        api_secret: "SEC789".into(),
        voice: "x3_xiaoyan".into(),
    };
    existing.generation.max_concurrent_calls = 7;
    existing.write_atomic(&paths.config_file).unwrap();

    let (mut app, _rx) = make_app(paths.clone(), progress, existing);

    app.open_wizard(WizardOrigin::Command);
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.draft.llm.base_url = server.uri();
        w.draft.llm.api_key = "sk-new".into();
        w.draft.llm.model = "new-model".into();
        w.step = WizardStep::Model;
    }
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::ConnectivityOk));

    let saved = Config::load(&paths.config_file).unwrap();
    assert_eq!(saved.llm.base_url, server.uri());
    assert_eq!(saved.tts.iflytek.app_id, "APP123");
    assert_eq!(saved.tts.iflytek.voice, "x3_xiaoyan");
    assert_eq!(saved.generation.max_concurrent_calls, 7);
}

#[tokio::test]
async fn config_command_opens_wizard_with_command_origin() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let mut cfg = Config::default();
    cfg.llm.api_key = "sk-existing".into();
    let (mut app, _rx) = make_app(paths, progress, cfg);

    // Simulate palette `/config`: drive the app through palette open + type + enter.
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    app.on_input(Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )));
    for c in "config".chars() {
        app.on_input(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    app.on_input(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));

    assert!(matches!(app.screen, Screen::ConfigWizard));
    let w = app.config_wizard.as_ref().unwrap();
    assert_eq!(w.origin, WizardOrigin::Command);
    // api_key was pre-seeded into draft and should be the initial input when ApiKey step is reached.
    assert_eq!(w.draft.llm.api_key, "sk-existing");
}

#[tokio::test]
async fn esc_on_endpoint_command_origin_aborts_wizard() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let mut cfg = Config::default();
    cfg.llm.api_key = "sk-existing".into();
    let (mut app, _rx) = make_app(paths, progress, cfg);

    app.open_wizard(WizardOrigin::Command);
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));

    assert!(matches!(app.screen, Screen::Study));
    assert!(app.config_wizard.is_none());
}
```

- [ ] **Step 2: Ensure `DataPaths::resolve` accepts `Option<&Path>` — verify signature**

Run: `grep -n "pub fn resolve" src/storage/paths.rs`

Expected output shows: `pub fn resolve(cli: Option<&Path>) -> anyhow::Result<Self>` (or similar). If the signature differs, adjust `setup()` helper accordingly.

- [ ] **Step 3: Run integration tests**

Run: `cargo test --test config_wizard`
Expected: 6 tests PASS

- [ ] **Step 4: Run full suite**

Run: `cargo test`
Expected: all PASS (existing + new)

- [ ] **Step 5: Commit**

```bash
git add tests/config_wizard.rs
git commit -m "test(config_wizard): add integration tests for wizard flow"
```

---

## Task 9: Final verification

- [ ] **Step 1: Full test run**

Run: `cargo test`
Expected: all tests PASS, no new warnings.

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Release build**

Run: `cargo build --release`
Expected: succeeds.

- [ ] **Step 4: Confirm working tree clean**

Run: `git status`
Expected: `nothing to commit, working tree clean`.

- [ ] **Step 5: Manual smoke (optional, requires real API key)**

From a shell with credentials:

```bash
rm -rf /tmp/inkworm-test
INKWORM_LLM_BASE_URL="https://api.chatanywhere.tech/v1" \
INKWORM_LLM_API_KEY="<user-supplied>" \
INKWORM_LLM_MODEL="gpt-4o-mini" \
cargo run -- --config /tmp/inkworm-test
```

Verify: wizard appears, 3 steps complete, connectivity check passes, app drops into Study (empty — no courses yet).

Then run again: wizard does NOT appear (config valid).

Then `/config` from palette: wizard appears with Command origin, existing values pre-filled.

---

## Self-Review Checklist

**Spec coverage:**
- [x] §3 Triggers: Task 7 (main.rs) + Task 7 (execute_command "config")
- [x] §4 WizardState: Task 2
- [x] §5 Key bindings: Task 5
- [x] §6 Per-step logic: Task 2 (commit/back) + Task 5 (event dispatch)
- [x] §7 Rendering + masking: Task 6
- [x] §8 Connectivity probe: Task 3 (probe_llm) + Task 5 (spawn + dispatch)
- [x] §9 Atomic save preserving fields: Task 5 (handle_wizard_task_msg re-reads + patches)
- [x] §10 Config validate split: Task 1
- [x] §11 App state changes: Task 4
- [x] §12 UX edge cases: covered by Task 5 (testing guard, Ctrl+C, backspace) + Task 6 (masking, blink reuse)
- [x] §13 Testing: Task 2 (step transitions) + Task 3 (probe) + Task 6 (render/mask) + Task 8 (integration)

**Placeholder scan:** none (no TBD/TODO markers besides the transient Task 3→5 handoff that Task 5 Step 5 explicitly removes).

**Type consistency:**
- `WizardStep` / `WizardOrigin` / `WizardState` / `CommitOutcome` / `BackOutcome` / `TestingState` defined in Task 2, used consistently in Tasks 3/5/6/8.
- `TaskMsg::Wizard(WizardTaskMsg)` added in Task 3, matched in Task 5, constructed in Task 8.
- `probe_llm(LlmConfig, CancellationToken) -> Result<(), AppError>` defined in Task 3, called from Task 5 `spawn_connectivity_test`.
- `render_config_wizard(frame, state, cursor_visible)` defined in Task 6, called from Task 4 Step 7 (render arm).
- `App::open_wizard(WizardOrigin)` defined in Task 4, called from Task 7 (execute_command) and main.rs.
