# Plan 7: Robustness + Polish — Design Spec

> **Status**: Design
> **Date**: 2026-04-22
> **Parent spec**: `2026-04-21-inkworm-design.md` §7.6, §8.3, §10.4

## 0. Scope

Five features to complete v1:

1. **Tracing / log-file wiring** — structured logging to `inkworm.log`
2. **`/logs` command** — show log file path + pbcopy
3. **`/doctor` command** — local-only health check overlay
4. **3-strikes session-disable** — 3 consecutive TTS failures → NullSpeaker for session
5. **Graceful `speaker.cancel()` on quit** — cancel in-flight speak before terminal restoration

---

## 1. Tracing / Log-File Wiring

### 1.1 Dependencies

Add to `Cargo.toml`:
```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"
```

### 1.2 Initialization

In `main.rs`, before any other work:

```rust
fn init_tracing(log_dir: &Path) {
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

Called after `DataPaths::resolve()` but before config load, so config errors are logged.

### 1.3 What to Log

- **Startup/shutdown**: `tracing::info!("inkworm starting")`, `tracing::info!("inkworm shutting down")`
- **Screen transitions**: `tracing::debug!("screen -> {:?}", screen)`
- **LLM calls**: url, model, attempt, duration_ms, result (already in reflexion.rs — add `tracing::info!`)
- **TTS calls**: text_hash (not text), cache_hit, duration_ms
- **Errors**: full error chain via `tracing::error!`

### 1.4 What NEVER to Log

- `api_key`, `api_secret` — mask in Display impls
- Article raw text
- Course content

### 1.5 No Log Rotation

v1 does not rotate logs. `/logs` command gives path; user manages manually.

---

## 2. `/logs` Command

### 2.1 Behavior

1. Copy log file path to clipboard via `pbcopy` (macOS only)
2. Show brief banner: "Log path copied: ~/.config/inkworm/inkworm.log"
3. Return to Study screen after 2 seconds (or immediately on keypress)

### 2.2 Implementation

- No new Screen variant needed — use the existing error_banner with `Severity::Info`
- `execute_command` match arm for `"logs"` calls a helper that spawns `pbcopy` via `std::process::Command`
- Set palette `available: true` for logs command

---

## 3. `/doctor` Command

### 3.1 Checks (local only)

| Check | Pass | Fail |
|-------|------|------|
| Config file exists | ✓ | ✗ path not found |
| LLM API key set | ✓ | ✗ empty |
| LLM endpoint reachable | (skip) | (skip) |
| TTS enabled | ✓ enabled / ✗ disabled | — |
| TTS creds set | ✓ | ✗ missing app_id/api_key/api_secret |
| Cache dir writable | ✓ | ✗ permission denied |
| Audio device available | ✓ (device name) | ✗ no device |

### 3.2 UI

New `Screen::Doctor` variant. Centered overlay (same pattern as `/tts` status and `/list`). Shows check results with ✓/✗ indicators. ESC to close.

### 3.3 New File

`src/ui/doctor.rs` — `pub fn run_checks(config, data_paths, speaker, device) -> Vec<CheckResult>` + `pub fn render_doctor(frame, results)`.

`CheckResult` is a simple struct: `{ label: String, ok: bool, detail: String }`.

---

## 4. 3-Strikes Session-Disable

### 4.1 State

`App` gains two fields:
```rust
pub tts_failure_count: u32,
pub tts_session_disabled: bool,
```

### 4.2 Logic

In `speak_current_drill`, after the spawned task captures an error:
- Increment `tts_failure_count` (via shared counter or task message)
- If `tts_failure_count >= 3`, set `tts_session_disabled = true`
- On success, reset `tts_failure_count = 0`

When `tts_session_disabled` is true, `speak_current_drill` returns early (same as NullSpeaker behavior). No need to actually swap the speaker — just guard the entry point.

### 4.3 Messaging

Use `TaskMsg` to send TTS results back to main loop (avoids shared mutable state):

```rust
pub enum TaskMsg {
    // ... existing variants
    TtsSpeakResult(Result<(), String>),
}
```

App handles `TtsSpeakResult`:
- `Ok(())` → reset `tts_failure_count = 0`
- `Err(msg)` → `tts_failure_count += 1`, `last_tts_error = Some(msg)`, check threshold

### 4.4 `/tts` Status Integration

The `/tts` status overlay should show "Session disabled (3 consecutive failures)" when `tts_session_disabled` is true.

---

## 5. Graceful `speaker.cancel()` on Quit

### 5.1 Current Problem

`quit()` saves progress and sets `should_quit = true`, but doesn't cancel in-flight TTS. Audio may trail during terminal restoration.

### 5.2 Fix

In `App::quit()`, add `self.speaker.cancel()` before setting `should_quit`:

```rust
fn quit(&mut self) {
    self.speaker.cancel();
    let _ = self.study.progress().save(&self.data_paths.progress_file);
    self.should_quit = true;
}
```

This is a one-line fix.

---

## 6. Files Changed

| File | Change |
|------|--------|
| `Cargo.toml` | Add tracing, tracing-subscriber, tracing-appender |
| `src/main.rs` | `init_tracing()` call |
| `src/app.rs` | 3-strikes fields, `TtsSpeakResult` handling, quit cancel, `/logs` + `/doctor` routing |
| `src/ui/doctor.rs` | New: health check logic + render |
| `src/ui/tts_status.rs` | Show session-disabled state |
| `src/ui/mod.rs` | Register doctor module |
| `src/ui/palette.rs` | Set `available: true` for logs/doctor |
| `src/ui/task_msg.rs` | Add `TtsSpeakResult` variant |
| `src/llm/reflexion.rs` | Add `tracing::info!` to LLM calls |
| `src/tts/iflytek.rs` | Add `tracing::info!` to TTS calls |

---

## 7. Out of Scope

- Log rotation (v2)
- Remote connectivity probes in `/doctor` (use `/config` wizard for that)
- `/tts on` to reset 3-strikes (session-level only, per spec §7.6)
- Windows/Linux support
