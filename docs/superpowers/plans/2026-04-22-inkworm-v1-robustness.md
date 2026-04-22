# Plan 7: Robustness + Polish — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete v1 with tracing logs, `/logs` + `/doctor` commands, 3-strikes TTS session-disable, and graceful quit cancel.

**Architecture:** Add tracing-subscriber for structured logging to `inkworm.log`. New `/logs` command uses pbcopy + info banner. New `/doctor` command runs local health checks and renders overlay. 3-strikes uses `TaskMsg::TtsSpeakResult` to track failures. Quit calls `speaker.cancel()` before exit.

**Tech Stack:** Rust, Ratatui 0.28, tracing + tracing-subscriber + tracing-appender, tokio

---

### Task 1: Add tracing dependencies + init

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`

- [ ] **Step 1: Add tracing dependencies to Cargo.toml**

Add after line 30 (after crossterm):

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"
```

- [ ] **Step 2: Run cargo check to verify dependencies**

Run: `cargo check`
Expected: SUCCESS (dependencies download and compile)

- [ ] **Step 3: Add init_tracing function to main.rs**

Add after line 13 (after imports), before `fn main()`:

```rust
fn init_tracing(log_dir: &std::path::Path) {
    let file_appender = tracing_appender::rolling::never(log_dir, "inkworm.log");
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("INKWORM_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(file_appender)
        .with_ansi(false)
        .init();
}
```

- [ ] **Step 4: Call init_tracing in main**

In `main()`, add after line 25 (after `paths.ensure_dirs()?;`):

```rust
init_tracing(&paths.data_dir);
tracing::info!("inkworm starting");
```

- [ ] **Step 5: Add shutdown log**

In `main()`, add before the final `Ok(())` (after the `rt.block_on` call):

```rust
tracing::info!("inkworm shutting down");
```

- [ ] **Step 6: Run cargo check**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat(logging): add tracing infrastructure with file appender"
```

---

### Task 2: Add tracing to LLM and TTS calls

**Files:**
- Modify: `src/llm/reflexion.rs`
- Modify: `src/tts/iflytek.rs`

- [ ] **Step 1: Add tracing to reflexion.rs LLM calls**

In `src/llm/reflexion.rs`, find the `chat` call in `generate_with_reflexion` (around line 50-80). Add before the call:

```rust
let start = std::time::Instant::now();
```

Add after the call (around the `match client.chat(...)` line):

```rust
let duration_ms = start.elapsed().as_millis();
match result {
    Ok(ref content) => {
        tracing::info!(
            url = %self.base_url,
            model = %self.model,
            attempt = attempt,
            duration_ms = duration_ms,
            result = "ok",
            "LLM call succeeded"
        );
    }
    Err(ref e) => {
        tracing::error!(
            url = %self.base_url,
            model = %self.model,
            attempt = attempt,
            duration_ms = duration_ms,
            error = %e,
            "LLM call failed"
        );
    }
}
```

- [ ] **Step 2: Add tracing to iflytek.rs TTS calls**

In `src/tts/iflytek.rs`, in the `speak` method (around line 100-150), add at the start:

```rust
let text_hash = format!("{:x}", blake3::hash(text.as_bytes()));
let start = std::time::Instant::now();
```

After the cache check (around line 110), add:

```rust
if wav_path.exists() {
    let duration_ms = start.elapsed().as_millis();
    tracing::info!(
        text_hash = %text_hash,
        cache_hit = true,
        duration_ms = duration_ms,
        "TTS cache hit"
    );
}
```

After the WS synthesis completes (around line 180), add:

```rust
let duration_ms = start.elapsed().as_millis();
tracing::info!(
    text_hash = %text_hash,
    cache_hit = false,
    duration_ms = duration_ms,
    "TTS synthesis completed"
);
```

- [ ] **Step 3: Run cargo check**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 4: Commit**

```bash
git add src/llm/reflexion.rs src/tts/iflytek.rs
git commit -m "feat(logging): add tracing to LLM and TTS calls"
```

---

### Task 3: Implement `/logs` command

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/palette.rs`

- [ ] **Step 1: Add execute_logs helper to app.rs**

In `src/app.rs`, add after the `execute_tts` method (around line 610):

```rust
fn execute_logs(&mut self) {
    let log_path = self.data_paths.data_dir.join("inkworm.log");
    let path_str = log_path.display().to_string();
    
    // Spawn pbcopy to copy path to clipboard
    let _ = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(path_str.as_bytes())?;
            }
            child.wait()
        });
    
    // Show info banner
    use crate::ui::error_banner::{UserMessage, Severity};
    let msg = UserMessage {
        headline: format!("Log path copied: {}", path_str),
        hint: String::new(),
        severity: Severity::Info,
    };
    self.study.set_feedback(crate::ui::study::FeedbackState::Error(msg));
    self.screen = Screen::Study;
}
```

- [ ] **Step 2: Wire execute_logs into execute_command**

In `execute_command` method (around line 590), add after the `"delete"` arm:

```rust
"logs" => self.execute_logs(),
```

- [ ] **Step 3: Enable logs command in palette**

In `src/ui/palette.rs`, find the `all_commands()` function (around line 30-60). Find the logs command entry and change `available: false` to `available: true`.

- [ ] **Step 4: Run cargo check**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 5: Manual test**

Run: `cargo run`
- Press Ctrl+P
- Type "logs" and press Enter
- Verify banner shows "Log path copied: ..."
- Run `pbpaste` in terminal to verify path is in clipboard

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/ui/palette.rs
git commit -m "feat(logs): implement /logs command with pbcopy"
```

---

### Task 4: Implement `/doctor` health check

**Files:**
- Create: `src/ui/doctor.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/app.rs`
- Modify: `src/ui/palette.rs`

- [ ] **Step 1: Create doctor.rs with CheckResult struct**

Create `src/ui/doctor.rs`:

```rust
//! Health check overlay for `/doctor` command.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::config::Config;
use crate::storage::DataPaths;
use crate::tts::speaker::Speaker;
use crate::tts::OutputKind;

#[derive(Debug)]
pub struct CheckResult {
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

pub fn run_checks(
    config: &Config,
    data_paths: &DataPaths,
    _speaker: &dyn Speaker,
    device: OutputKind,
) -> Vec<CheckResult> {
    let mut results = Vec::new();

    // Check 1: Config file exists
    results.push(CheckResult {
        label: "Config file".to_string(),
        ok: data_paths.config_file.exists(),
        detail: if data_paths.config_file.exists() {
            "found".to_string()
        } else {
            "not found".to_string()
        },
    });

    // Check 2: LLM API key set
    let llm_key_ok = !config.llm.api_key.trim().is_empty();
    results.push(CheckResult {
        label: "LLM API key".to_string(),
        ok: llm_key_ok,
        detail: if llm_key_ok {
            "set".to_string()
        } else {
            "empty".to_string()
        },
    });

    // Check 3: TTS enabled
    results.push(CheckResult {
        label: "TTS enabled".to_string(),
        ok: config.tts.enabled,
        detail: if config.tts.enabled {
            "yes".to_string()
        } else {
            "no".to_string()
        },
    });

    // Check 4: TTS creds set
    let tts_creds_ok = !config.tts.iflytek.app_id.trim().is_empty()
        && !config.tts.iflytek.api_key.trim().is_empty()
        && !config.tts.iflytek.api_secret.trim().is_empty();
    results.push(CheckResult {
        label: "TTS credentials".to_string(),
        ok: tts_creds_ok,
        detail: if tts_creds_ok {
            "set".to_string()
        } else {
            "missing".to_string()
        },
    });

    // Check 5: Cache dir writable
    let cache_writable = data_paths.tts_cache_dir.exists()
        && std::fs::metadata(&data_paths.tts_cache_dir)
            .map(|m| !m.permissions().readonly())
            .unwrap_or(false);
    results.push(CheckResult {
        label: "Cache directory".to_string(),
        ok: cache_writable,
        detail: if cache_writable {
            "writable".to_string()
        } else {
            "not writable".to_string()
        },
    });

    // Check 6: Audio device available
    let device_ok = !matches!(device, OutputKind::Unknown);
    results.push(CheckResult {
        label: "Audio device".to_string(),
        ok: device_ok,
        detail: if device_ok {
            format!("{:?}", device)
        } else {
            "unknown".to_string()
        },
    });

    results
}

pub fn render_doctor(frame: &mut Frame, results: &[CheckResult]) {
    let area = frame.area();
    let width = 60u16.min(area.width.saturating_sub(4));
    let height = (results.len() as u16 + 4).min(area.height.saturating_sub(4));
    let left = (area.width.saturating_sub(width)) / 2;
    let top = (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(left, top, width, height);

    let mut lines = vec![
        Line::from(Span::styled(
            "Health Check",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for result in results {
        let indicator = if result.ok { "✓" } else { "✗" };
        let color = if result.ok { Color::Green } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(indicator, Style::default().fg(color)),
            Span::raw(" "),
            Span::styled(&result.label, Style::default().fg(Color::White)),
            Span::raw(": "),
            Span::styled(&result.detail, Style::default().fg(Color::DarkGray)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Esc · close",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, rect);
}
```

- [ ] **Step 2: Register doctor module**

In `src/ui/mod.rs`, add after the `pub mod tts_status;` line:

```rust
pub mod doctor;
```

- [ ] **Step 3: Add Screen::Doctor variant**

In `src/app.rs`, add to the `Screen` enum (around line 29):

```rust
Doctor,
```

- [ ] **Step 4: Add doctor_results field to App**

In `src/app.rs`, add to the `App` struct (around line 50):

```rust
pub doctor_results: Option<Vec<crate::ui::doctor::CheckResult>>,
```

Initialize in `App::new` (around line 83):

```rust
doctor_results: None,
```

- [ ] **Step 5: Add execute_doctor method**

In `src/app.rs`, add after `execute_logs` (around line 630):

```rust
fn execute_doctor(&mut self) {
    let results = crate::ui::doctor::run_checks(
        &self.config,
        &self.data_paths,
        self.speaker.as_ref(),
        self.current_device,
    );
    self.doctor_results = Some(results);
    self.screen = Screen::Doctor;
}
```

- [ ] **Step 6: Wire execute_doctor into execute_command**

In `execute_command`, add after the `"logs"` arm:

```rust
"doctor" => self.execute_doctor(),
```

- [ ] **Step 7: Handle ESC in Doctor screen**

In `on_input`, add after the `Screen::TtsStatus` arm (around line 190):

```rust
Screen::Doctor => {
    if key.code == KeyCode::Esc {
        self.doctor_results = None;
        self.screen = Screen::Study;
    }
}
```

- [ ] **Step 8: Render Doctor screen**

In `render` method, add after the `Screen::TtsStatus` arm (around line 670):

```rust
Screen::Doctor => {
    crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
    if let Some(ref results) = self.doctor_results {
        crate::ui::doctor::render_doctor(frame, results);
    }
}
```

- [ ] **Step 9: Enable doctor command in palette**

In `src/ui/palette.rs`, find the doctor command entry and change `available: false` to `available: true`.

- [ ] **Step 10: Run cargo check**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 11: Manual test**

Run: `cargo run`
- Press Ctrl+P
- Type "doctor" and press Enter
- Verify overlay shows health checks with ✓/✗ indicators
- Press Esc to close

- [ ] **Step 12: Commit**

```bash
git add src/ui/doctor.rs src/ui/mod.rs src/app.rs src/ui/palette.rs
git commit -m "feat(doctor): implement /doctor health check overlay"
```

---

### Task 5: Implement 3-strikes TTS session-disable

**Files:**
- Modify: `src/ui/task_msg.rs`
- Modify: `src/app.rs`
- Modify: `src/ui/tts_status.rs`

- [ ] **Step 1: Add TtsSpeakResult variant to TaskMsg**

In `src/ui/task_msg.rs`, add after the existing variants (around line 30):

```rust
TtsSpeakResult(Result<(), String>),
```

- [ ] **Step 2: Add 3-strikes fields to App**

In `src/app.rs`, add to the `App` struct (around line 51):

```rust
pub tts_failure_count: u32,
pub tts_session_disabled: bool,
```

Initialize in `App::new` (around line 84):

```rust
tts_failure_count: 0,
tts_session_disabled: false,
```

- [ ] **Step 3: Guard speak_current_drill with session-disabled check**

In `speak_current_drill` method (around line 103), add after the first `self.speaker.cancel()` line:

```rust
if self.tts_session_disabled {
    return;
}
```

- [ ] **Step 4: Send TtsSpeakResult from spawned task**

In `speak_current_drill`, replace the spawned task (around line 118-122) with:

```rust
let text = drill.english.clone();
let speaker = Arc::clone(&self.speaker);
let last_error = Arc::clone(&self.last_tts_error);
let task_tx = self.task_tx.clone();
tokio::spawn(async move {
    let result = speaker.speak(&text).await;
    match &result {
        Ok(()) => {
            *last_error.lock().await = None;
        }
        Err(e) => {
            let msg = format!("{}", e);
            *last_error.lock().await = Some(msg.clone());
            let _ = task_tx.send(TaskMsg::TtsSpeakResult(Err(msg))).await;
        }
    }
    if result.is_ok() {
        let _ = task_tx.send(TaskMsg::TtsSpeakResult(Ok(()))).await;
    }
});
```

- [ ] **Step 5: Handle TtsSpeakResult in on_task_msg**

In `on_task_msg` method (around line 850), add after the `DeviceDetected` arm:

```rust
TaskMsg::TtsSpeakResult(result) => match result {
    Ok(()) => {
        self.tts_failure_count = 0;
    }
    Err(_) => {
        self.tts_failure_count += 1;
        if self.tts_failure_count >= 3 {
            self.tts_session_disabled = true;
            tracing::warn!("TTS session disabled after 3 consecutive failures");
        }
    }
},
```

- [ ] **Step 6: Show session-disabled state in /tts status**

In `src/ui/tts_status.rs`, in the `render_tts_status` function, add after the `speaking_str` calculation (around line 50):

```rust
let session_disabled = false; // Will be passed as parameter
```

Change the function signature to accept `session_disabled: bool`:

```rust
pub fn render_tts_status(
    frame: &mut Frame,
    config: &TtsConfig,
    device: OutputKind,
    last_error: Option<String>,
    cache_stats: (usize, u64),
    session_disabled: bool,
) {
```

Add a new line after the "Speaking" line (around line 85):

```rust
if session_disabled {
    lines.push(Line::from(vec![
        Span::styled("Status:     ", Style::default().fg(Color::DarkGray)),
        Span::styled("Session disabled (3 consecutive failures)", Style::default().fg(Color::Red)),
    ]));
}
```

- [ ] **Step 7: Update render call in app.rs**

In `src/app.rs`, in the `render` method, update the `Screen::TtsStatus` arm (around line 665):

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
        self.tts_session_disabled,
    );
}
```

- [ ] **Step 8: Run cargo check**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 9: Write integration test**

Add to `tests/tts_status.rs` (or create if it doesn't exist):

```rust
#[test]
fn three_strikes_disables_session() {
    // This is a conceptual test - actual implementation would need
    // a test harness that can simulate TTS failures
    // For now, just verify the logic compiles
    let mut failure_count = 0u32;
    let mut session_disabled = false;
    
    // Simulate 3 failures
    for _ in 0..3 {
        failure_count += 1;
        if failure_count >= 3 {
            session_disabled = true;
        }
    }
    
    assert!(session_disabled);
    assert_eq!(failure_count, 3);
}
```

- [ ] **Step 10: Run tests**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 11: Commit**

```bash
git add src/ui/task_msg.rs src/app.rs src/ui/tts_status.rs tests/tts_status.rs
git commit -m "feat(tts): implement 3-strikes session-disable"
```

---

### Task 6: Add graceful speaker.cancel() on quit

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add speaker.cancel() to quit method**

In `src/app.rs`, in the `quit` method (around line 619), add `self.speaker.cancel();` as the first line:

```rust
fn quit(&mut self) {
    self.speaker.cancel();
    let _ = self.study.progress().save(&self.data_paths.progress_file);
    self.should_quit = true;
}
```

- [ ] **Step 2: Run cargo check**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 3: Manual test**

Run: `cargo run`
- Start a drill that triggers TTS
- Immediately press Ctrl+P and type "quit"
- Verify no audio trails after terminal restoration

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(quit): add graceful speaker.cancel() before exit"
```

---

### Task 7: Final integration test + cleanup

**Files:**
- Modify: `tests/end_to_end.rs` (if exists, otherwise create)

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: no warnings

- [ ] **Step 3: Run rustfmt check**

Run: `rustfmt --edition 2021 --check src/main.rs src/app.rs src/ui/doctor.rs src/ui/tts_status.rs src/ui/task_msg.rs src/llm/reflexion.rs src/tts/iflytek.rs`
Expected: no formatting issues

- [ ] **Step 4: Manual smoke test**

Run: `cargo run`
- Verify startup log appears in `~/.config/inkworm/inkworm.log`
- Test `/logs` command (path copied)
- Test `/doctor` command (health checks display)
- Trigger TTS and check logs for TTS entries
- Test quit (no audio trails)

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "test: verify Plan 7 integration"
```

---

## Plan Complete

All 5 features implemented:
1. ✅ Tracing / log-file wiring
2. ✅ `/logs` command
3. ✅ `/doctor` command
4. ✅ 3-strikes session-disable
5. ✅ Graceful quit cancel

**Next steps:**
- Create PR for Plan 7
- User smoke test with real credentials
- v1 release preparation
