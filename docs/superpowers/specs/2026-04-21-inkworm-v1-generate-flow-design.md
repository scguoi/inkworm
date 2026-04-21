# inkworm Plan 4a: Generate Flow — Design Spec

> **Status**: Design (awaiting review)
> **Date**: 2026-04-21
> **Scope**: /import Generate screen, task_rx channel, error banner, /delete command
> **Parent spec**: `2026-04-21-inkworm-design.md` §8.7, §10.2

---

## 0. Goal

Enable the end-to-end user flow: paste an article → LLM generates a course → start typing. This is the missing link between the LLM pipeline (Plan 2) and the TUI (Plan 3).

---

## 1. New & Modified Modules

```
src/
├── app.rs               # [MODIFY] Screen::Generate, task_rx, delete confirm
├── ui/
│   ├── mod.rs           # [MODIFY] add generate, error_banner
│   ├── event.rs         # [MODIFY] select! third channel (task_rx)
│   ├── generate.rs      # [CREATE] Generate screen (Pasting/Running/Result)
│   └── error_banner.rs  # [CREATE] AppError → UserMessage mapping
├── llm/
│   └── reflexion.rs     # [MODIFY] generate() accepts progress sender
```

### Dependencies (new)

None — all needed crates (tokio mpsc, tokio_util CancellationToken) are already in Cargo.toml.

---

## 2. Event Loop Extension

The `select!` loop gains a third channel for background task messages:

```rust
loop {
    terminal.draw(|f| app.render(f))?;
    tokio::select! {
        Some(Ok(evt)) = crossterm_stream.next() => app.on_input(evt),
        _ = tick.tick() => app.on_tick(),
        Some(msg) = task_rx.recv() => app.on_task_msg(msg),
    }
    if app.should_quit { break; }
}
```

`task_rx` is `mpsc::Receiver<TaskMsg>`. `App` holds the `mpsc::Sender<TaskMsg>` for spawning background tasks.

```rust
enum TaskMsg {
    Generate(GenerateProgress),
}
```

---

## 3. GenerateProgress

```rust
pub enum GenerateProgress {
    Phase1Started,
    Phase1Done { sentence_count: usize },
    Phase2Progress { done: usize, total: usize },
    Done(Course),
    Failed(AppError),
}
```

Sent from the background tokio task running `Reflexion::generate` to the main loop via `mpsc`.

---

## 4. Reflexion::generate Progress Integration

Modify `Reflexion::generate` to accept an optional progress sender:

```rust
pub async fn generate(
    &self,
    article: &str,
    progress_tx: Option<mpsc::Sender<GenerateProgress>>,
) -> Result<Course, AppError>
```

- `None`: behavior unchanged (smoke test, tests)
- `Some(tx)`: sends `Phase1Started`, `Phase1Done`, `Phase2Progress` at appropriate points
- `Done` and `Failed` are NOT sent by generate itself — the spawning code wraps the Result

The existing `generate` call sites (smoke example, tests) pass `None`.

---

## 5. Generate Screen

### 5.1 State

```rust
pub enum GenerateSubstate {
    Pasting(PastingState),
    Running(RunningState),
    Result(ResultState),
}

pub struct PastingState {
    pub text: String,
    pub cursor_pos: usize,
}

pub struct RunningState {
    pub phase_label: String,
    pub done: usize,
    pub total: usize,
    pub cancel_token: CancellationToken,
}

pub struct ResultState {
    pub success: bool,
    pub error_msg: Option<UserMessage>,
    pub article_text: String,  // preserved for retry
}
```

### 5.2 Pasting Substate

- Full-screen textarea (70% height), status bar at bottom showing byte count / word count (whitespace-split) / limit
- Only this substate accepts `Event::Paste` (bracketed paste); all other screens discard paste events
- `Ctrl+Enter` submits (prevents conflict with pasted newlines)
- Submit is disabled (grayed out in status bar) when `text.len() > config.generation.max_article_bytes`
- `Esc` returns to Study screen
- Regular typing: printable chars append, `Backspace` deletes, `Enter` inserts newline

### 5.3 Running Substate

- On submit: spawn tokio task that calls `Reflexion::generate` with progress sender
- Display:
  - Phase 1: spinner + "Splitting article into sentences…"
  - Phase 2: progress bar + "Generating drills: {done}/{total}"
- Bottom right: `Esc · cancel`
- `Esc` triggers `CancellationToken::cancel()`, returns to Pasting (text preserved)
- No other keys respond during Running

### 5.4 Result Substate

- **Success**: save Course to `courses/` dir, update `progress.active_course_id`, transition to Study screen with the new course loaded
- **Failure**: display red error banner (via `error_banner::user_message`), show `r retry / Esc back`
  - `r`: re-enter Running with same article text
  - `Esc`: return to Pasting (text preserved)

---

## 6. Error Banner

```rust
pub enum Severity {
    Error,
    Warning,
    Info,
}

pub struct UserMessage {
    pub headline: String,
    pub hint: String,
    pub severity: Severity,
}

pub fn user_message(err: &AppError) -> UserMessage
```

Maps every `AppError` variant to a user-friendly message:

| Variant | Headline | Hint |
|---|---|---|
| `Llm(Unauthorized)` | "Authentication failed" | "Check your API key in config" |
| `Llm(Network)` | "Network error" | "Check your internet connection" |
| `Llm(Timeout)` | "Request timed out" | "Try again or check your endpoint" |
| `Llm(Server)` | "Server error" | "The API returned an error, try again" |
| `Reflexion { .. }` | "Course generation failed" | "LLM couldn't produce valid output after 3 attempts" |
| `Io(..)` | "File system error" | "Check disk space and permissions" |
| `Cancelled` | "Cancelled" | "" |
| `Config(..)` | "Configuration error" | "Run /config to fix" |
| `Storage(..)` | "Storage error" | "Check data directory permissions" |

---

## 7. /delete Command

Triggered from command palette:

1. If no active course → show info banner "No active course to delete"
2. If active course exists → show confirm prompt: "Delete course [title]? (y/n)"
3. `y` → `storage::delete_course` + remove course from `progress.courses` + clear `active_course_id` + transition to empty Study state
4. `n` / `Esc` → cancel, return to Study

Implemented as `Screen::DeleteConfirm` — a simple overlay on the Study screen showing the confirm prompt. Only `y`, `n`, and `Esc` keys respond.

---

## 8. App State Changes

```rust
pub enum Screen {
    Study,
    Palette,
    Help,
    Generate,       // NEW
    DeleteConfirm,  // NEW
}
```

`App` gains:
- `task_tx: mpsc::Sender<TaskMsg>` — for spawning background tasks
- `generate: Option<GenerateSubstate>` — Generate screen state
- `config: Config` — needed for LLM client creation and article size limit

`App::new` takes `Config` as additional parameter.

---

## 9. Testing Strategy

| What | How |
|---|---|
| `GenerateSubstate` transitions | Unit test: Pasting → submit → Running; progress msgs → update; Done → Success |
| `error_banner::user_message` | Exhaustive test: iterate all `AppError` variants, assert non-empty headline |
| Reflexion progress channel | Wiremock integration test: verify progress events arrive in correct order |
| `/delete` flow | Integration test: create temp course, delete, verify file removed + progress updated |
| Paste event routing | Unit test: paste event in Pasting → text appended; paste in Study → ignored |
| Article size limit | Unit test: text exceeding limit → submit disabled |
| Cancel during Running | Unit test: cancel → returns to Pasting with text preserved |

---

## 10. Out of Scope (Plan 4b / Plan 5)

- ConfigWizard (`/config`)
- Course list (`/list`)
- `/logs`, `/doctor`, `/tts` commands
- TTS playback and device detection
- `audio_poll` channel
