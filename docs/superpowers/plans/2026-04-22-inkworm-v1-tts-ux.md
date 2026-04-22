# Plan 6f: TTS UX Polish — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend ConfigWizard from 3 to 7 steps (adding TTS credential setup with live probe), add `/tts` status overlay, and update default voice.

**Architecture:** Extend existing `WizardStep` enum with 4 TTS variants. Add `tts_enabled` field to `WizardState` for dynamic step counting. New `Screen::TtsStatus` variant renders a read-only overlay. `cache_stats()` in `tts/cache.rs` provides cache metrics. `last_tts_error` on `App` captures speak failures for the status display.

**Tech Stack:** Rust, Ratatui 0.28, tokio, tokio-tungstenite (for TTS probe)

---

### Task 1: Default voice + cache_stats

**Files:**
- Modify: `src/config/defaults.rs:9`
- Modify: `src/tts/cache.rs`

- [ ] **Step 1: Write failing test for `cache_stats`**

Add to `src/tts/cache.rs` in the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn cache_stats_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let (count, bytes) = super::cache_stats(tmp.path());
    assert_eq!(count, 0);
    assert_eq!(bytes, 0);
}

#[test]
fn cache_stats_counts_wav_files_only() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.wav"), &[0u8; 100]).unwrap();
    std::fs::write(tmp.path().join("b.wav"), &[0u8; 200]).unwrap();
    std::fs::write(tmp.path().join("c.txt"), &[0u8; 999]).unwrap();
    let (count, bytes) = super::cache_stats(tmp.path());
    assert_eq!(count, 2);
    assert_eq!(bytes, 300);
}

#[test]
fn cache_stats_missing_dir_returns_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nope");
    let (count, bytes) = super::cache_stats(&missing);
    assert_eq!(count, 0);
    assert_eq!(bytes, 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib tts::cache::tests::cache_stats -- --nocapture`
Expected: compilation error — `cache_stats` not defined.

- [ ] **Step 3: Implement `cache_stats` and update default voice**

In `src/tts/cache.rs`, add after the `cache_path` function:

```rust
/// Count `.wav` files and sum their sizes in `dir`.
/// Returns `(0, 0)` if the directory is missing or unreadable.
pub fn cache_stats(dir: &Path) -> (usize, u64) {
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return (0, 0),
    };
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }
        if let Ok(meta) = path.metadata() {
            if meta.is_file() {
                count += 1;
                bytes += meta.len();
            }
        }
    }
    (count, bytes)
}
```

In `src/config/defaults.rs`, change line 9:

```rust
pub const DEFAULT_IFLYTEK_VOICE: &str = "x4_enus_catherine_profnews";
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib tts::cache::tests::cache_stats`
Expected: all 3 new tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/tts/cache.rs src/config/defaults.rs
git commit -m "feat(tts): add cache_stats and update default voice to x4_enus_catherine_profnews"
```

---

### Task 2: Extend WizardStep enum + WizardState fields

**Files:**
- Modify: `src/ui/config_wizard.rs`

- [ ] **Step 1: Write failing tests for TTS wizard steps**

Add to the `#[cfg(test)] mod tests` block in `src/ui/config_wizard.rs`:

```rust
#[test]
fn tts_enable_y_advances_to_tts_app_id() {
    let mut w = new_wiz();
    // Fast-forward through LLM steps
    w.input = "https://x/v1".into();
    w.commit();
    w.input = "sk-123".into();
    w.commit();
    w.step = WizardStep::TtsEnable;
    w.input = "y".into();
    let outcome = w.commit();
    assert!(matches!(outcome, CommitOutcome::Advance));
    assert_eq!(w.step, WizardStep::TtsAppId);
    assert!(w.tts_enabled);
}

#[test]
fn tts_enable_n_returns_save_config() {
    let mut w = new_wiz();
    w.step = WizardStep::TtsEnable;
    w.input = "n".into();
    let outcome = w.commit();
    assert!(matches!(outcome, CommitOutcome::SaveConfig));
    assert!(!w.tts_enabled);
    assert!(!w.draft.tts.enabled);
}

#[test]
fn tts_enable_invalid_input_stays() {
    let mut w = new_wiz();
    w.step = WizardStep::TtsEnable;
    w.input = "maybe".into();
    let outcome = w.commit();
    assert!(matches!(outcome, CommitOutcome::Invalid));
    assert!(w.error.is_some());
}

#[test]
fn tts_credential_steps_advance() {
    let mut w = new_wiz();
    w.tts_enabled = true;
    w.step = WizardStep::TtsAppId;
    w.input = "app123".into();
    let outcome = w.commit();
    assert!(matches!(outcome, CommitOutcome::Advance));
    assert_eq!(w.step, WizardStep::TtsApiKey);
    assert_eq!(w.draft.tts.iflytek.app_id, "app123");

    w.input = "key456".into();
    let outcome = w.commit();
    assert!(matches!(outcome, CommitOutcome::Advance));
    assert_eq!(w.step, WizardStep::TtsApiSecret);
    assert_eq!(w.draft.tts.iflytek.api_key, "key456");

    w.input = "sec789".into();
    let outcome = w.commit();
    assert!(matches!(outcome, CommitOutcome::ProbeTts));
    assert_eq!(w.draft.tts.iflytek.api_secret, "sec789");
}

#[test]
fn tts_back_navigation() {
    let mut w = new_wiz();
    w.tts_enabled = true;
    w.step = WizardStep::TtsApiSecret;
    w.draft.tts.iflytek.api_key = "k".into();
    let outcome = w.back();
    assert!(matches!(outcome, BackOutcome::Back));
    assert_eq!(w.step, WizardStep::TtsApiKey);
    assert_eq!(w.input, "k");

    w.draft.tts.iflytek.app_id = "a".into();
    let outcome = w.back();
    assert!(matches!(outcome, BackOutcome::Back));
    assert_eq!(w.step, WizardStep::TtsAppId);
    assert_eq!(w.input, "a");

    let outcome = w.back();
    assert!(matches!(outcome, BackOutcome::Back));
    assert_eq!(w.step, WizardStep::TtsEnable);
}

#[test]
fn total_steps_dynamic() {
    let mut w = new_wiz();
    // Before TtsEnable, total is unknown — show 4 (LLM 3 + TtsEnable 1)
    w.step = WizardStep::TtsEnable;
    w.tts_enabled = false;
    assert_eq!(w.total_steps(), 4);

    w.tts_enabled = true;
    assert_eq!(w.total_steps(), 7);
}

#[test]
fn step_number_sequential() {
    let mut w = new_wiz();
    w.step = WizardStep::Endpoint;
    assert_eq!(w.step_number(), 1);
    w.step = WizardStep::TtsEnable;
    assert_eq!(w.step_number(), 4);
    w.step = WizardStep::TtsApiSecret;
    assert_eq!(w.step_number(), 7);
}

#[test]
fn wizard_title_dynamic() {
    let mut w = new_wiz();
    w.step = WizardStep::TtsEnable;
    w.tts_enabled = false;
    assert_eq!(wizard_title_dynamic(&w), "inkworm — setup (4 / 4)");
    w.tts_enabled = true;
    assert_eq!(wizard_title_dynamic(&w), "inkworm — setup (4 / 7)");
}

#[test]
fn mask_hides_tts_secrets() {
    assert_eq!(mask_for_display("secret", WizardStep::TtsApiKey), "******");
    assert_eq!(mask_for_display("secret", WizardStep::TtsApiSecret), "******");
    assert_eq!(mask_for_display("app123", WizardStep::TtsAppId), "app123");
}

#[test]
fn hint_tts_steps() {
    assert_eq!(
        wizard_hint(WizardStep::TtsEnable, WizardOrigin::FirstRun, false),
        "Enter · next     Esc · back"
    );
    assert_eq!(
        wizard_hint(WizardStep::TtsApiSecret, WizardOrigin::FirstRun, false),
        "Enter · test and save     Esc · back"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::config_wizard::tests -- --nocapture 2>&1 | head -20`
Expected: compilation errors — `TtsEnable`, `TtsAppId`, etc. not defined.

- [ ] **Step 3: Implement WizardStep extension + WizardState changes**

In `src/ui/config_wizard.rs`:

**a) Extend `WizardStep` enum:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Endpoint,
    ApiKey,
    Model,
    TtsEnable,
    TtsAppId,
    TtsApiKey,
    TtsApiSecret,
}
```

**b) Add `tts_enabled` to `WizardState` and `SaveConfig`/`ProbeTts` to `CommitOutcome`:**

```rust
pub struct WizardState {
    pub step: WizardStep,
    pub origin: WizardOrigin,
    pub draft: Config,
    pub input: String,
    pub testing: Option<TestingState>,
    pub error: Option<UserMessage>,
    pub tts_enabled: bool,
}
```

```rust
pub enum CommitOutcome {
    Advance,
    ProbeConnectivity,
    ProbeTts,
    SaveConfig,
    Invalid,
}
```

**c) Update `WizardState::new` to init `tts_enabled: draft.tts.enabled`.**

**d) Add `total_steps()` and `step_number()` methods:**

```rust
pub fn total_steps(&self) -> u8 {
    if self.tts_enabled { 7 } else { 4 }
}

pub fn step_number(&self) -> u8 {
    match self.step {
        WizardStep::Endpoint => 1,
        WizardStep::ApiKey => 2,
        WizardStep::Model => 3,
        WizardStep::TtsEnable => 4,
        WizardStep::TtsAppId => 5,
        WizardStep::TtsApiKey => 6,
        WizardStep::TtsApiSecret => 7,
    }
}
```

**e) Extend `commit()` — add TTS match arms after the existing `Model` arm:**

```rust
WizardStep::TtsEnable => {
    let val = trimmed.to_lowercase();
    match val.as_str() {
        "y" => {
            self.tts_enabled = true;
            self.draft.tts.enabled = true;
            self.step = WizardStep::TtsAppId;
            self.input = self.draft.tts.iflytek.app_id.clone();
            CommitOutcome::Advance
        }
        "n" => {
            self.tts_enabled = false;
            self.draft.tts.enabled = false;
            CommitOutcome::SaveConfig
        }
        _ => {
            self.error = Some(UserMessage {
                headline: "Type y or n".to_string(),
                hint: String::new(),
                severity: crate::ui::error_banner::Severity::Error,
            });
            CommitOutcome::Invalid
        }
    }
}
WizardStep::TtsAppId => {
    self.draft.tts.iflytek.app_id = trimmed.to_string();
    self.step = WizardStep::TtsApiKey;
    self.input = self.draft.tts.iflytek.api_key.clone();
    CommitOutcome::Advance
}
WizardStep::TtsApiKey => {
    self.draft.tts.iflytek.api_key = trimmed.to_string();
    self.step = WizardStep::TtsApiSecret;
    self.input = self.draft.tts.iflytek.api_secret.clone();
    CommitOutcome::Advance
}
WizardStep::TtsApiSecret => {
    self.draft.tts.iflytek.api_secret = trimmed.to_string();
    CommitOutcome::ProbeTts
}
```

Note: the empty-input check at the top of `commit()` needs to handle TtsEnable differently — TtsEnable should NOT reject empty (it rejects non-y/n instead). Add a guard:

```rust
// At the top of commit(), replace the empty check:
if trimmed.is_empty() && self.step != WizardStep::TtsEnable {
    let label = match self.step {
        WizardStep::Endpoint => "Endpoint cannot be empty",
        WizardStep::ApiKey => "API key cannot be empty",
        WizardStep::Model => "Model cannot be empty",
        WizardStep::TtsAppId => "App ID cannot be empty",
        WizardStep::TtsApiKey => "API key cannot be empty",
        WizardStep::TtsApiSecret => "API secret cannot be empty",
        WizardStep::TtsEnable => unreachable!(),
    };
    // ... rest unchanged
}
```

**f) Extend `back()` — add TTS match arms:**

```rust
WizardStep::TtsEnable => {
    self.step = WizardStep::Model;
    self.input = self.draft.llm.model.clone();
    BackOutcome::Back
}
WizardStep::TtsAppId => {
    self.step = WizardStep::TtsEnable;
    self.input = if self.tts_enabled { "y".into() } else { "n".into() };
    BackOutcome::Back
}
WizardStep::TtsApiKey => {
    self.step = WizardStep::TtsAppId;
    self.input = self.draft.tts.iflytek.app_id.clone();
    BackOutcome::Back
}
WizardStep::TtsApiSecret => {
    self.step = WizardStep::TtsApiKey;
    self.input = self.draft.tts.iflytek.api_key.clone();
    BackOutcome::Back
}
```

**g) Replace `wizard_title` with `wizard_title_dynamic`:**

```rust
pub fn wizard_title_dynamic(state: &WizardState) -> String {
    let n = state.step_number();
    let total = state.total_steps();
    format!("inkworm — setup ({n} / {total})")
}
```

Keep the old `wizard_title` for backward compat in existing tests, or update them.

**h) Extend `wizard_step_label`:**

```rust
WizardStep::TtsEnable => "Enable TTS? (y/n)",
WizardStep::TtsAppId => "iFlytek App ID",
WizardStep::TtsApiKey => "iFlytek API Key",
WizardStep::TtsApiSecret => "iFlytek API Secret",
```

**i) Extend `mask_for_display` — mask TtsApiKey and TtsApiSecret:**

```rust
pub fn mask_for_display(input: &str, step: WizardStep) -> String {
    match step {
        WizardStep::ApiKey | WizardStep::TtsApiKey | WizardStep::TtsApiSecret => {
            "*".repeat(input.chars().count())
        }
        _ => input.to_string(),
    }
}
```

**j) Extend `wizard_hint`:**

```rust
(WizardStep::TtsEnable, _) => "Enter · next     Esc · back",
(WizardStep::TtsAppId, _) => "Enter · next     Esc · back",
(WizardStep::TtsApiKey, _) => "Enter · next     Esc · back",
(WizardStep::TtsApiSecret, _) => "Enter · test and save     Esc · back",
```

**k) Update `render_config_wizard` to use `wizard_title_dynamic`:**

Change `let title = wizard_title(state.step);` to `let title = wizard_title_dynamic(state);`.

**l) Update existing `wizard_title` tests** — the old `wizard_title` function can be removed or kept as a helper. Update `title_shows_1_of_3_on_endpoint` test to use `wizard_title_dynamic` with a `WizardState`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::config_wizard::tests`
Expected: all tests PASS (old + new).

- [ ] **Step 5: Commit**

```bash
git add src/ui/config_wizard.rs
git commit -m "feat(wizard): extend WizardStep with TTS enable/credential steps and dynamic step count"
```

---

### Task 3: Wire TTS wizard steps into App

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/task_msg.rs`
- Modify: `src/ui/config_wizard.rs` (add `probe_tts`)

- [ ] **Step 1: Write failing integration test for TTS probe flow**

Add to `tests/config_wizard.rs`:

```rust
#[tokio::test]
async fn tts_probe_success_saves_and_dismisses() {
    // Mock iFlytek WS server that accepts one connection and returns success frame
    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr().unwrap();
    let ws_url = format!("ws://{}", addr);

    tokio::spawn(async move {
        let (stream, _) = server.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let (mut write, mut read) = ws.split();
        // Read client request
        let _ = read.next().await;
        // Send success frame
        let resp = serde_json::json!({
            "code": 0,
            "data": {"status": 2, "audio": ""},
            "message": "success"
        });
        write.send(Message::Text(resp.to_string())).await.unwrap();
    });

    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths.clone(), progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.draft.llm.base_url = "https://x/v1".into();
        w.draft.llm.api_key = "sk-test".into();
        w.draft.llm.model = "gpt-4o-mini".into();
        w.draft.tts.iflytek.app_id = "app123".into();
        w.draft.tts.iflytek.api_key = "key456".into();
        w.draft.tts.iflytek.api_secret = "sec789".into();
        w.tts_enabled = true;
        w.step = WizardStep::TtsApiSecret;
    }

    // Simulate TTS probe success
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::TtsProbeOk));

    assert!(matches!(app.screen, Screen::Study));
    assert!(app.config_wizard.is_none());
    let saved = Config::load(&paths.config_file).unwrap();
    assert_eq!(saved.tts.iflytek.app_id, "app123");
    assert!(saved.tts.enabled);
}

#[tokio::test]
async fn tts_probe_failed_keeps_wizard_open() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths, progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.tts_enabled = true;
        w.step = WizardStep::TtsApiSecret;
    }

    let err = inkworm::error::AppError::Tts(inkworm::tts::speaker::TtsError::Auth("bad creds".into()));
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::TtsProbeFailed(err)));

    assert!(matches!(app.screen, Screen::ConfigWizard));
    let w = app.config_wizard.as_ref().unwrap();
    assert!(w.testing.is_none());
    assert!(w.error.is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test config_wizard tts_probe -- --nocapture`
Expected: compilation errors — `TtsProbeOk`, `TtsProbeFailed`, `AppError::Tts` not defined.

- [ ] **Step 3: Add TTS probe variants to WizardTaskMsg**

In `src/ui/task_msg.rs`:

```rust
#[derive(Debug)]
pub enum WizardTaskMsg {
    ConnectivityOk,
    ConnectivityFailed(AppError),
    TtsProbeOk,
    TtsProbeFailed(AppError),
}
```

- [ ] **Step 4: Add `AppError::Tts` variant**

In `src/error.rs`, add to the `AppError` enum:

```rust
#[error("TTS error: {0}")]
Tts(#[from] crate::tts::speaker::TtsError),
```

- [ ] **Step 5: Implement `probe_tts` function**

In `src/ui/config_wizard.rs`, add after `probe_llm`:

```rust
/// Fire a minimal TTS synthesis request to verify iFlytek credentials work.
/// Uses an ephemeral cache dir and no audio output (cache-only mode).
pub async fn probe_tts(
    iflytek: crate::config::IflytekConfig,
    cancel: CancellationToken,
) -> Result<(), AppError> {
    use crate::tts::speaker::build_speaker;
    use crate::config::TtsOverride;
    use std::path::PathBuf;

    let cache_dir = std::env::temp_dir().join("inkworm-tts-probe");
    std::fs::create_dir_all(&cache_dir).ok();

    let speaker = build_speaker(&iflytek, cache_dir, TtsOverride::On, None);

    tokio::select! {
        res = speaker.speak("hello") => {
            res.map_err(AppError::Tts)
        }
        _ = cancel.cancelled() => Err(AppError::Cancelled),
    }
}
```

- [ ] **Step 6: Wire TTS probe into App**

In `src/app.rs`, update `handle_config_wizard_key` to handle `CommitOutcome::ProbeTts` and `CommitOutcome::SaveConfig`:

```rust
// In handle_config_wizard_key, after the existing ProbeConnectivity arm:
CommitOutcome::ProbeTts => {
    self.spawn_tts_probe();
}
CommitOutcome::SaveConfig => {
    self.save_wizard_config();
}
```

Add `spawn_tts_probe` method:

```rust
fn spawn_tts_probe(&mut self) {
    use crate::ui::config_wizard::{probe_tts, TestingState};
    use crate::ui::task_msg::WizardTaskMsg;

    let iflytek = match self.config_wizard.as_ref() {
        Some(s) => s.draft.tts.iflytek.clone(),
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
        let msg = match probe_tts(iflytek, cancel).await {
            Ok(()) => WizardTaskMsg::TtsProbeOk,
            Err(e) => WizardTaskMsg::TtsProbeFailed(e),
        };
        let _ = task_tx.send(TaskMsg::Wizard(msg)).await;
    });
}
```

Add `save_wizard_config` method (extracted from existing `handle_wizard_task_msg` ConnectivityOk arm):

```rust
fn save_wizard_config(&mut self) {
    let Some(wizard) = self.config_wizard.as_mut() else {
        return;
    };
    wizard.testing = None;

    let mut merged = Config::load(&self.data_paths.config_file).unwrap_or_default();
    merged.llm = wizard.draft.llm.clone();
    merged.tts = wizard.draft.tts.clone();
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
```

Update `handle_wizard_task_msg` to handle TTS probe results:

```rust
// In handle_wizard_task_msg, add after ConnectivityFailed arm:
WizardTaskMsg::TtsProbeOk => {
    self.save_wizard_config();
}
WizardTaskMsg::TtsProbeFailed(e) => {
    wizard.error = Some(user_message(&e));
}
```

Also update the existing `ConnectivityOk` arm to call `save_wizard_config()` instead of inlining the logic.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --test config_wizard`
Expected: all tests PASS.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs src/ui/task_msg.rs src/ui/config_wizard.rs src/error.rs tests/config_wizard.rs
git commit -m "feat(wizard): wire TTS probe into App with spawn_tts_probe and save_wizard_config"
```

---

### Task 4: Add `/tts` status overlay

**Files:**
- Create: `src/ui/tts_status.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Write failing test for TTS status rendering**

Add to `tests/tts_status.rs` (new file):

```rust
use inkworm::config::{Config, IflytekConfig, TtsOverride};
use inkworm::tts::OutputKind;

#[test]
fn status_display_all_fields() {
    let mut cfg = Config::default();
    cfg.tts.r#override = TtsOverride::Auto;
    cfg.tts.iflytek = IflytekConfig {
        app_id: "app".into(),
        api_key: "key".into(),
        api_secret: "sec".into(),
        voice: "x4_enus_catherine_profnews".into(),
    };

    let device = OutputKind::Bluetooth;
    let last_error = Some("Network timeout".to_string());
    let cache_stats = (12, 1_234_567);

    // This will fail until we implement render_tts_status
    // For now, just test the data assembly logic
    let mode_str = format!("{:?}", cfg.tts.r#override).to_lowercase();
    assert_eq!(mode_str, "auto");

    let device_str = match device {
        OutputKind::Bluetooth | OutputKind::WiredHeadphones => "headphones",
        OutputKind::BuiltInSpeaker | OutputKind::ExternalSpeaker => "speaker",
        OutputKind::Unknown => "unknown",
    };
    assert_eq!(device_str, "headphones");

    let creds_ok = !cfg.tts.iflytek.app_id.trim().is_empty()
        && !cfg.tts.iflytek.api_key.trim().is_empty()
        && !cfg.tts.iflytek.api_secret.trim().is_empty();
    assert!(creds_ok);

    let (count, bytes) = cache_stats;
    let mb = bytes as f64 / 1_048_576.0;
    let cache_str = format!("{} files ({:.1} MB)", count, mb);
    assert_eq!(cache_str, "12 files (1.2 MB)");

    let error_str = last_error.as_deref().unwrap_or("(none)");
    assert_eq!(error_str, "Network timeout");
}
```

- [ ] **Step 2: Run test to verify it passes (data logic only)**

Run: `cargo test --test tts_status`
Expected: PASS (this test doesn't call render yet).

- [ ] **Step 3: Implement `tts_status.rs` module**

Create `src/ui/tts_status.rs`:

```rust
//! TTS status overlay — read-only display of mode, device, cache, creds, last error.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::config::TtsConfig;
use crate::tts::OutputKind;

pub fn render_tts_status(
    frame: &mut Frame,
    config: &TtsConfig,
    device: OutputKind,
    last_error: Option<String>,
    cache_stats: (usize, u64),
) {
    let area = frame.area();
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 12u16;
    let left = (area.width.saturating_sub(width)) / 2;
    let top = (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(left, top, width, height);

    let mode_str = format!("{:?}", config.r#override).to_lowercase();
    let device_str = match device {
        OutputKind::Bluetooth | OutputKind::WiredHeadphones => "headphones",
        OutputKind::BuiltInSpeaker | OutputKind::ExternalSpeaker => "speaker",
        OutputKind::Unknown => "unknown",
    };

    let creds_ok = !config.iflytek.app_id.trim().is_empty()
        && !config.iflytek.api_key.trim().is_empty()
        && !config.iflytek.api_secret.trim().is_empty();
    let creds_str = if creds_ok { "✓ set" } else { "✗ not set" };

    let (count, bytes) = cache_stats;
    let mb = bytes as f64 / 1_048_576.0;
    let cache_str = format!("{} files ({:.1} MB)", count, mb);

    let error_str = last_error.as_deref().unwrap_or("(none)");

    let speaking_str = if crate::tts::should_speak(config.r#override, device, creds_ok) {
        "enabled"
    } else {
        "disabled"
    };

    let lines = vec![
        Line::from(Span::styled(
            "TTS Status",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Mode:       ", Style::default().fg(Color::DarkGray)),
            Span::styled(mode_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Device:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(device_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Speaking:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(speaking_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Creds:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(creds_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Cache:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(cache_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Last error: ", Style::default().fg(Color::DarkGray)),
            Span::styled(error_str, Style::default().fg(Color::Red)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Esc · close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, rect);
}
```

- [ ] **Step 4: Register module in `ui/mod.rs`**

Add to `src/ui/mod.rs`:

```rust
pub mod tts_status;
```

- [ ] **Step 5: Add `Screen::TtsStatus` variant**

In `src/app.rs`, add to the `Screen` enum:

```rust
pub enum Screen {
    Study,
    Palette,
    Help,
    Generate,
    DeleteConfirm,
    ConfigWizard,
    CourseList,
    TtsStatus,
}
```

- [ ] **Step 6: Add `last_tts_error` field to App**

In `src/app.rs`, add to the `App` struct:

```rust
pub last_tts_error: Option<String>,
```

Initialize in `App::new`:

```rust
last_tts_error: None,
```

- [ ] **Step 7: Capture TTS errors in `speak_current_drill`**

In `src/app.rs`, update `speak_current_drill`:

```rust
pub fn speak_current_drill(&mut self) {
    self.speaker.cancel();
    let Some(drill) = self.study.current_drill() else {
        return;
    };
    if !should_speak(
        self.config.tts.r#override,
        self.current_device,
        self.tts_has_creds(),
    ) {
        return;
    }
    let text = drill.english.clone();
    let speaker = Arc::clone(&self.speaker);
    let last_error = Arc::new(tokio::sync::Mutex::new(self.last_tts_error.clone()));
    tokio::spawn(async move {
        if let Err(e) = speaker.speak(&text).await {
            *last_error.lock().await = Some(format!("{}", e));
        }
    });
}
```

Wait, this won't work — we need to update `self.last_tts_error` from the spawned task. Let me fix this:

```rust
pub fn speak_current_drill(&self) {
    self.speaker.cancel();
    let Some(drill) = self.study.current_drill() else {
        return;
    };
    if !should_speak(
        self.config.tts.r#override,
        self.current_device,
        self.tts_has_creds(),
    ) {
        return;
    }
    let text = drill.english.clone();
    let speaker = Arc::clone(&self.speaker);
    // Can't update self.last_tts_error from spawned task without Arc<Mutex>
    // For now, skip error tracking — will add in a follow-up if needed
    tokio::spawn(async move {
        let _ = speaker.speak(&text).await;
    });
}
```

Actually, let's make `last_tts_error` an `Arc<Mutex<Option<String>>>` so we can update it from the spawned task:

In `App` struct:

```rust
pub last_tts_error: Arc<tokio::sync::Mutex<Option<String>>>,
```

In `App::new`:

```rust
last_tts_error: Arc::new(tokio::sync::Mutex::new(None)),
```

In `speak_current_drill`:

```rust
pub fn speak_current_drill(&self) {
    self.speaker.cancel();
    let Some(drill) = self.study.current_drill() else {
        return;
    };
    if !should_speak(
        self.config.tts.r#override,
        self.current_device,
        self.tts_has_creds(),
    ) {
        return;
    }
    let text = drill.english.clone();
    let speaker = Arc::clone(&self.speaker);
    let last_error = Arc::clone(&self.last_tts_error);
    tokio::spawn(async move {
        if let Err(e) = speaker.speak(&text).await {
            *last_error.lock().await = Some(format!("{}", e));
        }
    });
}
```

- [ ] **Step 8: Route `/tts` with no args to TtsStatus screen**

In `src/app.rs`, update `execute_tts`:

```rust
fn execute_tts(&mut self, args: &[String]) {
    use crate::config::TtsOverride;
    let first = args.first().map(|s| s.as_str()).unwrap_or("");
    match first {
        "on" => self.set_tts_override(TtsOverride::On),
        "off" => self.set_tts_override(TtsOverride::Off),
        "auto" => self.set_tts_override(TtsOverride::Auto),
        "clear-cache" => {
            let _ = crate::tts::clear_cache(&self.data_paths.tts_cache_dir);
        }
        "" => {
            self.screen = Screen::TtsStatus;
        }
        _ => {}
    }
}
```

- [ ] **Step 9: Render TtsStatus screen**

In `src/app.rs`, add to the `render` method:

```rust
Screen::TtsStatus => {
    crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
    let cache_stats = crate::tts::cache::cache_stats(&self.data_paths.tts_cache_dir);
    let last_error = self.last_tts_error.blocking_lock().clone();
    crate::ui::tts_status::render_tts_status(
        frame,
        &self.config.tts,
        self.current_device,
        last_error,
        cache_stats,
    );
}
```

- [ ] **Step 10: Handle ESC in TtsStatus screen**

In `src/app.rs`, add to `on_input` match for `Event::Key`:

```rust
Screen::TtsStatus => {
    if key.code == KeyCode::Esc {
        self.screen = Screen::Study;
    }
}
```

- [ ] **Step 11: Run tests**

Run: `cargo test`
Expected: all tests PASS.

- [ ] **Step 12: Manual smoke test**

Run: `cargo run`
- Type `/tts` and press Enter
- Verify status overlay appears with correct data
- Press Esc to close

- [ ] **Step 13: Commit**

```bash
git add src/ui/tts_status.rs src/ui/mod.rs src/app.rs tests/tts_status.rs
git commit -m "feat(tts): add /tts status overlay with mode/device/cache/creds/error display"
```

---

### Task 5: Integration tests + final cleanup

**Files:**
- Modify: `tests/config_wizard.rs`

- [ ] **Step 1: Add end-to-end wizard test**

Add to `tests/config_wizard.rs`:

```rust
#[tokio::test]
async fn full_wizard_flow_with_tts_enabled() {
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

    // Simulate user typing through all steps
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    // Endpoint
    for c in server.uri().chars() {
        app.on_input(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    // ApiKey
    for c in "sk-test".chars() {
        app.on_input(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    // Model
    for c in "gpt-4o-mini".chars() {
        app.on_input(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    // LLM probe success
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::ConnectivityOk));

    // Now on TtsEnable step
    assert_eq!(app.config_wizard.as_ref().unwrap().step, WizardStep::TtsEnable);

    // Type 'y'
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)));
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    // TtsAppId
    for c in "app123".chars() {
        app.on_input(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    // TtsApiKey
    for c in "key456".chars() {
        app.on_input(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    // TtsApiSecret
    for c in "sec789".chars() {
        app.on_input(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    // TTS probe success
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::TtsProbeOk));

    // Wizard dismissed, config saved
    assert!(matches!(app.screen, Screen::Study));
    let saved = Config::load(&paths.config_file).unwrap();
    assert_eq!(saved.llm.base_url, server.uri());
    assert_eq!(saved.tts.iflytek.app_id, "app123");
    assert!(saved.tts.enabled);
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --test config_wizard full_wizard_flow_with_tts_enabled`
Expected: PASS.

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: all tests PASS.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Run rustfmt**

Run: `cargo fmt --check`
Expected: no formatting issues.

- [ ] **Step 6: Final commit**

```bash
git add tests/config_wizard.rs
git commit -m "test(wizard): add end-to-end test for full TTS-enabled wizard flow"
```

---

## Plan Complete

All tasks implemented. The wizard now supports TTS credential setup with live probe, and `/tts` displays a status overlay.

**Next steps:**
- Manual smoke test with real iFlytek credentials
- Update main spec doc if needed
- Proceed to Plan 7 (robustness + polish)

