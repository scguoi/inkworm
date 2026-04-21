# Generate Flow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable end-to-end user flow: paste article → LLM generates course → start typing

**Architecture:** Three-channel event loop (crossterm + tick + task_rx), Generate screen with three substates (Pasting/Running/Result), background tokio task for LLM generation with progress reporting via mpsc

**Tech Stack:** tokio mpsc, tokio_util CancellationToken, ratatui, crossterm bracketed paste

---

## File Structure

```
src/
├── app.rs               # [MODIFY] Screen enum + task_tx + generate state + config
├── ui/
│   ├── mod.rs           # [MODIFY] add task_msg, generate, error_banner
│   ├── event.rs         # [MODIFY] select! third channel (task_rx)
│   ├── task_msg.rs      # [CREATE] TaskMsg + GenerateProgress enums
│   ├── generate.rs      # [CREATE] Generate screen state + input + render
│   └── error_banner.rs  # [CREATE] AppError → UserMessage mapping
├── llm/
│   └── reflexion.rs     # [MODIFY] generate() accepts progress sender
└── main.rs              # [MODIFY] pass Config to App::new

tests/
└── generate.rs          # [CREATE] integration tests
```

---

## Task 1: TaskMsg + GenerateProgress types

**Files:**
- Create: `src/ui/task_msg.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create `src/ui/task_msg.rs`**

```rust
use crate::error::AppError;
use crate::storage::course::Course;

/// Messages sent from background tasks to the main event loop.
#[derive(Debug)]
pub enum TaskMsg {
    Generate(GenerateProgress),
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
```

- [ ] **Step 2: Register module in `src/ui/mod.rs`**

Add after existing module declarations:

```rust
pub mod task_msg;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add src/ui/task_msg.rs src/ui/mod.rs
git commit -m "feat(ui): add TaskMsg and GenerateProgress types"
```

---

## Task 2: Error banner

**Files:**
- Create: `src/ui/error_banner.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create `src/ui/error_banner.rs`**

```rust
use crate::error::AppError;
use crate::llm::error::LlmError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct UserMessage {
    pub headline: String,
    pub hint: String,
    pub severity: Severity,
}

/// Map AppError to user-friendly message.
pub fn user_message(err: &AppError) -> UserMessage {
    match err {
        AppError::Llm(llm_err) => match llm_err {
            LlmError::Unauthorized => UserMessage {
                headline: "Authentication failed".to_string(),
                hint: "Check your API key in config".to_string(),
                severity: Severity::Error,
            },
            LlmError::Network(_) => UserMessage {
                headline: "Network error".to_string(),
                hint: "Check your internet connection".to_string(),
                severity: Severity::Error,
            },
            LlmError::Timeout(_) => UserMessage {
                headline: "Request timed out".to_string(),
                hint: "Try again or check your endpoint".to_string(),
                severity: Severity::Error,
            },
            LlmError::RateLimited(_) => UserMessage {
                headline: "Rate limited".to_string(),
                hint: "Wait a moment and try again".to_string(),
                severity: Severity::Warning,
            },
            LlmError::Server { .. } => UserMessage {
                headline: "Server error".to_string(),
                hint: "The API returned an error, try again".to_string(),
                severity: Severity::Error,
            },
            LlmError::InvalidResponse(_) => UserMessage {
                headline: "Response parse error".to_string(),
                hint: "The API returned invalid data".to_string(),
                severity: Severity::Error,
            },
            LlmError::Cancelled => UserMessage {
                headline: "Cancelled".to_string(),
                hint: String::new(),
                severity: Severity::Info,
            },
        },
        AppError::Reflexion { .. } => UserMessage {
            headline: "Course generation failed".to_string(),
            hint: "LLM couldn't produce valid output after 3 attempts".to_string(),
            severity: Severity::Error,
        },
        AppError::Io(_) => UserMessage {
            headline: "File system error".to_string(),
            hint: "Check disk space and permissions".to_string(),
            severity: Severity::Error,
        },
        AppError::Cancelled => UserMessage {
            headline: "Cancelled".to_string(),
            hint: String::new(),
            severity: Severity::Info,
        },
        AppError::Config(_) => UserMessage {
            headline: "Configuration error".to_string(),
            hint: "Run /config to fix".to_string(),
            severity: Severity::Error,
        },
        AppError::Storage(_) => UserMessage {
            headline: "Storage error".to_string(),
            hint: "Check data directory permissions".to_string(),
            severity: Severity::Error,
        },
    }
}
```

- [ ] **Step 2: Write test for exhaustive coverage**

Add to end of `src/ui/error_banner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigError;
    use crate::storage::StorageError;
    use std::path::PathBuf;

    #[test]
    fn all_variants_have_non_empty_headline() {
        let cases: Vec<AppError> = vec![
            AppError::Llm(LlmError::Unauthorized),
            AppError::Llm(LlmError::Network(reqwest::Error::from(std::io::Error::new(std::io::ErrorKind::Other, "test")))),
            AppError::Llm(LlmError::Timeout(std::time::Duration::from_secs(30))),
            AppError::Llm(LlmError::RateLimited(None)),
            AppError::Llm(LlmError::Server { status: 500, body: "test".into() }),
            AppError::Llm(LlmError::InvalidResponse("test".into())),
            AppError::Llm(LlmError::Cancelled),
            AppError::Reflexion {
                attempts: 3,
                saved_to: PathBuf::from("/tmp/test"),
                summary: "test".into(),
            },
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, "test")),
            AppError::Cancelled,
            AppError::Config(ConfigError::MissingField("test")),
            AppError::Storage(StorageError::NotFound("test".into())),
        ];

        for err in cases {
            let msg = user_message(&err);
            assert!(!msg.headline.is_empty(), "Empty headline for {:?}", err);
        }
    }
}
```

- [ ] **Step 3: Register module in `src/ui/mod.rs`**

Add:

```rust
pub mod error_banner;
```

- [ ] **Step 4: Run test**

Run: `cargo test --lib ui::error_banner`
Expected: 1 test PASS

- [ ] **Step 5: Commit**

```bash
git add src/ui/error_banner.rs src/ui/mod.rs
git commit -m "feat(ui): add error banner with AppError mapping"
```

---
## Task 3: Generate screen state (no rendering yet)

**Files:**
- Create: `src/ui/generate.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create `src/ui/generate.rs` with state types**

```rust
use tokio_util::sync::CancellationToken;

use crate::ui::error_banner::UserMessage;

#[derive(Debug)]
pub enum GenerateSubstate {
    Pasting(PastingState),
    Running(RunningState),
    Result(ResultState),
}

#[derive(Debug)]
pub struct PastingState {
    pub text: String,
    pub cursor_pos: usize,
}

#[derive(Debug)]
pub struct RunningState {
    pub phase_label: String,
    pub done: usize,
    pub total: usize,
    pub cancel_token: CancellationToken,
}

#[derive(Debug)]
pub struct ResultState {
    pub success: bool,
    pub error_msg: Option<UserMessage>,
    pub article_text: String,
}

impl PastingState {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor_pos: 0,
        }
    }

    pub fn byte_count(&self) -> usize {
        self.text.len()
    }

    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }

    pub fn can_submit(&self, max_bytes: usize) -> bool {
        !self.text.trim().is_empty() && self.text.len() <= max_bytes
    }

    pub fn type_char(&mut self, c: char) {
        self.text.push(c);
    }

    pub fn backspace(&mut self) {
        self.text.pop();
    }
}

impl RunningState {
    pub fn new() -> Self {
        Self {
            phase_label: "Starting...".to_string(),
            done: 0,
            total: 0,
            cancel_token: CancellationToken::new(),
        }
    }
}

impl ResultState {
    pub fn success() -> Self {
        Self {
            success: true,
            error_msg: None,
            article_text: String::new(),
        }
    }

    pub fn failure(error_msg: UserMessage, article_text: String) -> Self {
        Self {
            success: false,
            error_msg: Some(error_msg),
            article_text,
        }
    }
}
```

- [ ] **Step 2: Add unit tests**

Append to `src/ui/generate.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasting_byte_and_word_count() {
        let mut state = PastingState::new();
        state.text = "hello world".to_string();
        assert_eq!(state.byte_count(), 11);
        assert_eq!(state.word_count(), 2);
    }

    #[test]
    fn can_submit_requires_non_empty_and_under_limit() {
        let mut state = PastingState::new();
        assert!(!state.can_submit(100));
        state.text = "test".to_string();
        assert!(state.can_submit(100));
        state.text = "a".repeat(101);
        assert!(!state.can_submit(100));
    }

    #[test]
    fn type_and_backspace() {
        let mut state = PastingState::new();
        state.type_char('a');
        state.type_char('b');
        assert_eq!(state.text, "ab");
        state.backspace();
        assert_eq!(state.text, "a");
    }
}
```

- [ ] **Step 3: Register module**

Add to `src/ui/mod.rs`:

```rust
pub mod generate;
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib ui::generate`
Expected: 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/ui/generate.rs src/ui/mod.rs
git commit -m "feat(ui): add Generate screen state types"
```

---

## Task 4: Reflexion progress integration

**Files:**
- Modify: `src/llm/reflexion.rs`

- [ ] **Step 1: Add progress_tx parameter to generate()**

Find the `generate` function signature (around line 215) and modify:

```rust
pub async fn generate(
    &self,
    article: &str,
    existing_ids: &[String],
    progress_tx: Option<tokio::sync::mpsc::Sender<crate::ui::task_msg::GenerateProgress>>,
) -> Result<ReflexionOutcome, ReflexionError> {
```

- [ ] **Step 2: Send Phase1Started before reflexion_split**

After the function signature, add:

```rust
if let Some(tx) = &progress_tx {
    let _ = tx.send(crate::ui::task_msg::GenerateProgress::Phase1Started).await;
}
```

- [ ] **Step 3: Send Phase1Done after reflexion_split**

After `let phase1 = self.reflexion_split(article).await?;`, add:

```rust
if let Some(tx) = &progress_tx {
    let _ = tx.send(crate::ui::task_msg::GenerateProgress::Phase1Done {
        sentence_count: phase1.sentences.len(),
    }).await;
}
```

- [ ] **Step 4: Modify orchestrate_phase2 to accept progress_tx**

Change the signature of `orchestrate_phase2` (around line 190):

```rust
pub async fn orchestrate_phase2(
    &self,
    sentences: &[RawSentence],
    progress_tx: Option<&tokio::sync::mpsc::Sender<crate::ui::task_msg::GenerateProgress>>,
) -> Result<Vec<RawDrills>, ReflexionError> {
```

- [ ] **Step 5: Send Phase2Progress in orchestrate_phase2**

Replace the `try_join_all(tasks).await` line with:

```rust
let mut results = Vec::with_capacity(sentences.len());
for (i, task) in tasks.into_iter().enumerate() {
    let result = task.await?;
    results.push(result);
    if let Some(tx) = progress_tx {
        let _ = tx.send(crate::ui::task_msg::GenerateProgress::Phase2Progress {
            done: i + 1,
            total: sentences.len(),
        }).await;
    }
}
Ok(results)
```

- [ ] **Step 6: Update generate() call to orchestrate_phase2**

Change the line `let phase2 = self.orchestrate_phase2(&phase1.sentences).await?;` to:

```rust
let phase2 = self.orchestrate_phase2(&phase1.sentences, progress_tx.as_ref()).await?;
```

- [ ] **Step 7: Update existing call sites to pass None**

Find the smoke example and tests that call `generate()`. Add `, None` as the last parameter.

Search for calls:

```bash
grep -rn "\.generate(" examples/ tests/ --include="*.rs"
```

Update each call site to add `, None` parameter.

- [ ] **Step 8: Verify compilation**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 9: Run existing tests**

Run: `cargo test llm::reflexion`
Expected: all tests PASS

- [ ] **Step 10: Commit**

```bash
git add src/llm/reflexion.rs examples/ tests/
git commit -m "feat(llm): add progress reporting to Reflexion::generate"
```

---
## Task 5: App state extension

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add Screen variants**

Find the `Screen` enum (around line 11) and add:

```rust
pub enum Screen {
    Study,
    Palette,
    Help,
    Generate,       // NEW
    DeleteConfirm,  // NEW
}
```

- [ ] **Step 2: Add fields to App struct**

Find the `App` struct (around line 17) and add these fields:

```rust
pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
    pub study: StudyState,
    pub palette: Option<PaletteState>,
    pub data_paths: DataPaths,
    pub clock: Box<dyn Clock>,
    blink_counter: u32,
    pub cursor_visible: bool,
    // NEW fields:
    pub task_tx: tokio::sync::mpsc::Sender<crate::ui::task_msg::TaskMsg>,
    pub generate: Option<crate::ui::generate::GenerateSubstate>,
    pub config: crate::config::Config,
    pub delete_confirming: Option<String>,  // course title being confirmed for deletion
}
```

- [ ] **Step 3: Update App::new signature**

Change the `new` function signature (around line 28):

```rust
pub fn new(
    course: Option<Course>,
    progress: Progress,
    data_paths: DataPaths,
    clock: Box<dyn Clock>,
    config: crate::config::Config,
    task_tx: tokio::sync::mpsc::Sender<crate::ui::task_msg::TaskMsg>,
) -> Self {
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
    }
}
```

- [ ] **Step 4: Add on_task_msg method**

Add after the `on_input` method:

```rust
pub fn on_task_msg(&mut self, msg: crate::ui::task_msg::TaskMsg) {
    use crate::ui::task_msg::{TaskMsg, GenerateProgress};
    use crate::ui::generate::{GenerateSubstate, RunningState, ResultState};
    use crate::ui::error_banner::user_message;

    match msg {
        TaskMsg::Generate(progress) => {
            match progress {
                GenerateProgress::Phase1Started => {
                    if let Some(GenerateSubstate::Running(ref mut state)) = self.generate {
                        state.phase_label = "Splitting article into sentences…".to_string();
                    }
                }
                GenerateProgress::Phase1Done { sentence_count } => {
                    if let Some(GenerateSubstate::Running(ref mut state)) = self.generate {
                        state.phase_label = "Generating drills…".to_string();
                        state.total = sentence_count;
                    }
                }
                GenerateProgress::Phase2Progress { done, total } => {
                    if let Some(GenerateSubstate::Running(ref mut state)) = self.generate {
                        state.done = done;
                        state.total = total;
                        state.phase_label = format!("Generating drills: {done}/{total}");
                    }
                }
                GenerateProgress::Done(course) => {
                    // Save course and transition to Study
                    let course_id = course.id.clone();
                    if let Err(e) = crate::storage::course::save_course(&self.data_paths.courses_dir, &course) {
                        let article_text = if let Some(GenerateSubstate::Running(ref state)) = self.generate {
                            // Preserve article text from somewhere - for now use empty
                            String::new()
                        } else {
                            String::new()
                        };
                        self.generate = Some(GenerateSubstate::Result(ResultState::failure(
                            user_message(&crate::error::AppError::Storage(e)),
                            article_text,
                        )));
                        return;
                    }
                    // Update progress
                    self.study.progress_mut().active_course_id = Some(course_id.clone());
                    let _ = self.study.progress().save(&self.data_paths.progress_file);
                    // Load course into study
                    self.study = StudyState::new(Some(course), self.study.progress().clone());
                    self.generate = None;
                    self.screen = Screen::Study;
                }
                GenerateProgress::Failed(err) => {
                    let article_text = if let Some(GenerateSubstate::Running(ref state)) = self.generate {
                        String::new()  // TODO: preserve from spawn
                    } else {
                        String::new()
                    };
                    self.generate = Some(GenerateSubstate::Result(ResultState::failure(
                        user_message(&err),
                        article_text,
                    )));
                }
            }
        }
    }
}
```

- [ ] **Step 5: Add handle_generate_key method**

Add after `handle_help_key`:

```rust
fn handle_generate_key(&mut self, key: crossterm::event::KeyEvent) {
    use crossterm::event::{KeyCode, KeyModifiers};
    use crate::ui::generate::{GenerateSubstate, PastingState, RunningState};

    let Some(ref mut gen_state) = self.generate else { return };

    match gen_state {
        GenerateSubstate::Pasting(ref mut pasting) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('c') => self.quit(),
                    KeyCode::Enter => {
                        // Submit
                        if pasting.can_submit(self.config.generation.max_article_bytes) {
                            self.submit_generate(pasting.text.clone());
                        }
                    }
                    _ => {}
                }
                return;
            }
            match key.code {
                KeyCode::Esc => {
                    self.generate = None;
                    self.screen = Screen::Study;
                }
                KeyCode::Char(c) => pasting.type_char(c),
                KeyCode::Backspace => pasting.backspace(),
                KeyCode::Enter => pasting.type_char('\n'),
                _ => {}
            }
        }
        GenerateSubstate::Running(ref running) => {
            if key.code == KeyCode::Esc {
                running.cancel_token.cancel();
                // Return to Pasting - need to preserve text
                self.generate = Some(GenerateSubstate::Pasting(PastingState::new()));
            }
        }
        GenerateSubstate::Result(ref result) => {
            match key.code {
                KeyCode::Char('r') if !result.success => {
                    // Retry
                    self.submit_generate(result.article_text.clone());
                }
                KeyCode::Esc => {
                    if result.success {
                        self.generate = None;
                        self.screen = Screen::Study;
                    } else {
                        // Return to Pasting with preserved text
                        let mut pasting = PastingState::new();
                        pasting.text = result.article_text.clone();
                        self.generate = Some(GenerateSubstate::Pasting(pasting));
                    }
                }
                _ => {}
            }
        }
    }
}

fn submit_generate(&mut self, article: String) {
    use crate::ui::generate::{GenerateSubstate, RunningState};
    use crate::ui::task_msg::{TaskMsg, GenerateProgress};
    use crate::llm::client::ReqwestClient;
    use crate::llm::reflexion::Reflexion;

    let running = RunningState::new();
    let cancel_token = running.cancel_token.clone();
    self.generate = Some(GenerateSubstate::Running(running));

    let task_tx = self.task_tx.clone();
    let config = self.config.clone();
    let data_paths = self.data_paths.clone();
    let clock = self.clock.clone_box();
    let existing_ids: Vec<String> = self.study.progress().courses.keys().cloned().collect();

    tokio::spawn(async move {
        let client = ReqwestClient::new(
            config.llm.base_url.clone(),
            config.llm.api_key.clone(),
            std::time::Duration::from_secs(config.llm.request_timeout_secs),
        )?;
        let reflexion = Reflexion {
            client: &client,
            clock: clock.as_ref(),
            paths: &data_paths,
            model: &config.llm.model,
            max_concurrent: config.generation.max_concurrent_calls,
            cancel: cancel_token,
        };

        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel(16);
        let progress_tx_clone = progress_tx.clone();

        // Spawn progress forwarder
        tokio::spawn(async move {
            while let Some(progress) = progress_rx.recv().await {
                let _ = task_tx.send(TaskMsg::Generate(progress)).await;
            }
        });

        match reflexion.generate(&article, &existing_ids, Some(progress_tx_clone)).await {
            Ok(outcome) => {
                let _ = task_tx.send(TaskMsg::Generate(GenerateProgress::Done(outcome.course))).await;
            }
            Err(e) => {
                let app_err = match e {
                    crate::llm::reflexion::ReflexionError::Llm(llm_err) => {
                        crate::error::AppError::Llm(llm_err)
                    }
                    crate::llm::reflexion::ReflexionError::Cancelled => {
                        crate::error::AppError::Cancelled
                    }
                    crate::llm::reflexion::ReflexionError::AllAttemptsFailed { saved_to, .. } => {
                        crate::error::AppError::Reflexion {
                            attempts: 3,
                            saved_to,
                            summary: "Generation failed".to_string(),
                        }
                    }
                    crate::llm::reflexion::ReflexionError::Storage(s) => {
                        crate::error::AppError::Storage(s)
                    }
                    crate::llm::reflexion::ReflexionError::BudgetExceeded => {
                        crate::error::AppError::Reflexion {
                            attempts: 0,
                            saved_to: std::path::PathBuf::new(),
                            summary: "Budget exceeded".to_string(),
                        }
                    }
                };
                let _ = task_tx.send(TaskMsg::Generate(GenerateProgress::Failed(app_err))).await;
            }
        }
    });
}
```

- [ ] **Step 6: Update on_input to handle Generate screen**

In the `on_input` method, add Generate case:

```rust
pub fn on_input(&mut self, event: Event) {
    if let Event::Key(key) = event {
        match &self.screen {
            Screen::Study => self.handle_study_key(key),
            Screen::Palette => self.handle_palette_key(key),
            Screen::Help => self.handle_help_key(key),
            Screen::Generate => self.handle_generate_key(key),  // NEW
            Screen::DeleteConfirm => self.handle_delete_confirm_key(key),  // NEW
        }
    } else if let Event::Paste(text) = event {
        // Only Generate Pasting accepts paste
        if let Screen::Generate = self.screen {
            if let Some(crate::ui::generate::GenerateSubstate::Pasting(ref mut pasting)) = self.generate {
                pasting.text.push_str(&text);
            }
        }
    }
}
```

- [ ] **Step 7: Update execute_command to handle import**

In the `execute_command` method, add import case:

```rust
fn execute_command(&mut self, cmd: &Command) {
    match cmd.name {
        "quit" | "q" => self.quit(),
        "skip" => self.study.skip(),
        "help" => self.screen = Screen::Help,
        "import" => {
            self.generate = Some(crate::ui::generate::GenerateSubstate::Pasting(
                crate::ui::generate::PastingState::new()
            ));
            self.screen = Screen::Generate;
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

- [ ] **Step 8: Add handle_delete_confirm_key**

```rust
fn handle_delete_confirm_key(&mut self, key: crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode;

    match key.code {
        KeyCode::Char('y') => {
            if let Some(title) = &self.delete_confirming {
                if let Some(course) = self.study.current_course() {
                    let course_id = course.id.clone();
                    // Delete course file
                    if let Err(e) = crate::storage::course::delete_course(&self.data_paths.courses_dir, &course_id) {
                        eprintln!("Failed to delete course: {e}");
                    }
                    // Remove from progress
                    self.study.progress_mut().courses.remove(&course_id);
                    self.study.progress_mut().active_course_id = None;
                    let _ = self.study.progress().save(&self.data_paths.progress_file);
                    // Transition to empty study
                    self.study = StudyState::new(None, self.study.progress().clone());
                }
            }
            self.delete_confirming = None;
            self.screen = Screen::Study;
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            self.delete_confirming = None;
            self.screen = Screen::Study;
        }
        _ => {}
    }
}
```

- [ ] **Step 9: Add StudyState::current_course and progress_mut methods**

This requires modifying `src/ui/study.rs`. Add these methods to `StudyState`:

```rust
pub fn current_course(&self) -> Option<&Course> {
    self.course.as_ref()
}

pub fn progress_mut(&mut self) -> &mut Progress {
    &mut self.progress
}
```

- [ ] **Step 10: Add Clock::clone_box method**

This requires modifying `src/clock.rs`. Add to the `Clock` trait:

```rust
fn clone_box(&self) -> Box<dyn Clock>;
```

And implement for `SystemClock` and `FixedClock`:

```rust
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn clone_box(&self) -> Box<dyn Clock> {
        Box::new(SystemClock)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }

    fn clone_box(&self) -> Box<dyn Clock> {
        Box::new(self.clone())
    }
}
```

- [ ] **Step 11: Update palette commands availability**

In `src/ui/palette.rs`, change `import` and `delete` to `available: true`:

```rust
Command { name: "import", aliases: &[], description: "Create a new course", available: true },
Command { name: "delete", aliases: &[], description: "Delete current course", available: true },
```

- [ ] **Step 12: Verify compilation**

Run: `cargo check`
Expected: compiles (may have warnings about unused methods)

- [ ] **Step 13: Commit**

```bash
git add src/app.rs src/ui/study.rs src/clock.rs src/ui/palette.rs
git commit -m "feat(app): add Generate and DeleteConfirm screens with task_rx handling"
```

---
## Task 6: Event loop extension

**Files:**
- Modify: `src/ui/event.rs`

- [ ] **Step 1: Add task_rx parameter to run_loop**

Change the function signature:

```rust
pub async fn run_loop(
    guard: &mut TerminalGuard,
    app: &mut App,
    mut task_rx: tokio::sync::mpsc::Receiver<crate::ui::task_msg::TaskMsg>,
) -> std::io::Result<()> {
```

- [ ] **Step 2: Add third channel to select!**

Replace the `tokio::select!` block:

```rust
tokio::select! {
    Some(Ok(evt)) = crossterm_stream.next() => app.on_input(evt),
    _ = tick.tick() => app.on_tick(),
    Some(msg) = task_rx.recv() => app.on_task_msg(msg),
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/ui/event.rs
git commit -m "feat(ui): add task_rx channel to event loop"
```

---

## Task 7: Update main.rs

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Create task channel and pass to App**

Find the `rt.block_on` section and modify:

```rust
rt.block_on(async {
    let mut guard = TerminalGuard::new()?;
    let (task_tx, task_rx) = tokio::sync::mpsc::channel(32);
    let mut app = App::new(course, progress, paths.clone(), Box::new(SystemClock), config, task_tx);
    run_loop(&mut guard, &mut app, task_rx).await
})?;
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: compiles

- [ ] **Step 3: Run existing tests**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(main): wire up task channel to App and event loop"
```

---

## Task 8: Generate screen rendering

**Files:**
- Modify: `src/ui/generate.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Add render function to generate.rs**

Append to `src/ui/generate.rs`:

```rust
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};

pub fn render_generate(frame: &mut Frame, state: &GenerateSubstate, max_bytes: usize) {
    let area = frame.area();

    match state {
        GenerateSubstate::Pasting(pasting) => {
            render_pasting(frame, area, pasting, max_bytes);
        }
        GenerateSubstate::Running(running) => {
            render_running(frame, area, running);
        }
        GenerateSubstate::Result(result) => {
            render_result(frame, area, result);
        }
    }
}

fn render_pasting(frame: &mut Frame, area: Rect, state: &PastingState, max_bytes: usize) {
    let text_height = (area.height * 70 / 100).max(5);
    let text_area = Rect::new(0, 0, area.width, text_height);
    let status_y = text_height;

    // Textarea
    let para = Paragraph::new(state.text.as_str())
        .block(Block::default().borders(Borders::ALL).title("Paste Article (Ctrl+Enter to submit)"))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, text_area);

    // Status bar
    let byte_count = state.byte_count();
    let word_count = state.word_count();
    let can_submit = state.can_submit(max_bytes);
    let status_color = if can_submit { Color::Green } else { Color::Red };
    let status_text = format!(
        "{} bytes / {} words / {} limit {}",
        byte_count,
        word_count,
        max_bytes,
        if can_submit { "✓" } else { "✗ exceeds limit" }
    );
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(status_color));
    frame.render_widget(status, Rect::new(0, status_y, area.width, 1));
}

fn render_running(frame: &mut Frame, area: Rect, state: &RunningState) {
    let y = area.height / 2;

    // Phase label
    let label = Paragraph::new(state.phase_label.as_str())
        .style(Style::default().fg(Color::Yellow))
        .centered();
    frame.render_widget(label, Rect::new(0, y.saturating_sub(2), area.width, 1));

    // Progress bar (if phase 2)
    if state.total > 0 {
        let ratio = state.done as f64 / state.total as f64;
        let gauge = Gauge::default()
            .ratio(ratio)
            .gauge_style(Style::default().fg(Color::Yellow))
            .label(format!("{}/{}", state.done, state.total));
        frame.render_widget(gauge, Rect::new(area.width / 4, y, area.width / 2, 1));
    }

    // Cancel hint
    let hint = Paragraph::new("Esc · cancel")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Right);
    frame.render_widget(hint, Rect::new(0, area.height.saturating_sub(1), area.width, 1));
}

fn render_result(frame: &mut Frame, area: Rect, state: &ResultState) {
    let y = area.height / 2;

    if state.success {
        let msg = Paragraph::new("Course created successfully!")
            .style(Style::default().fg(Color::Green))
            .centered();
        frame.render_widget(msg, Rect::new(0, y, area.width, 1));
    } else if let Some(ref error_msg) = state.error_msg {
        let color = match error_msg.severity {
            crate::ui::error_banner::Severity::Error => Color::Red,
            crate::ui::error_banner::Severity::Warning => Color::Yellow,
            crate::ui::error_banner::Severity::Info => Color::Blue,
        };
        let headline = Paragraph::new(error_msg.headline.as_str())
            .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
            .centered();
        frame.render_widget(headline, Rect::new(0, y.saturating_sub(1), area.width, 1));

        if !error_msg.hint.is_empty() {
            let hint = Paragraph::new(error_msg.hint.as_str())
                .style(Style::default().fg(Color::DarkGray))
                .centered();
            frame.render_widget(hint, Rect::new(0, y, area.width, 1));
        }

        let actions = Paragraph::new("r retry / Esc back")
            .style(Style::default().fg(Color::DarkGray))
            .centered();
        frame.render_widget(actions, Rect::new(0, y + 2, area.width, 1));
    }
}
```

- [ ] **Step 2: Update App::render to handle Generate screen**

In `src/app.rs`, find the `render` method and add Generate case:

```rust
pub fn render(&self, frame: &mut Frame) {
    match &self.screen {
        Screen::Study => crate::ui::study::render_study(frame, &self.study, self.cursor_visible),
        Screen::Palette => {
            crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
            if let Some(palette) = &self.palette {
                crate::ui::palette::render_palette(frame, palette);
            }
        }
        Screen::Help => crate::ui::palette::render_help(frame),
        Screen::Generate => {
            if let Some(ref gen_state) = self.generate {
                crate::ui::generate::render_generate(frame, gen_state, self.config.generation.max_article_bytes);
            }
        }
        Screen::DeleteConfirm => {
            crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
            if let Some(ref title) = self.delete_confirming {
                render_delete_confirm(frame, title);
            }
        }
    }
}

fn render_delete_confirm(frame: &mut Frame, title: &str) {
    use ratatui::{
        layout::Rect,
        style::{Color, Style},
        text::Line,
        widgets::Paragraph,
    };

    let area = frame.area();
    let y = area.height / 2;
    let msg = format!("Delete course \"{}\"? (y/n)", title);
    let para = Paragraph::new(Line::from(vec![
        ratatui::text::Span::styled(msg, Style::default().fg(Color::Yellow)),
    ]))
    .centered();
    frame.render_widget(para, Rect::new(0, y, area.width, 1));
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/ui/generate.rs src/app.rs
git commit -m "feat(ui): add Generate screen rendering (Pasting/Running/Result)"
```

---


## Task 9: Integration tests

**Files:**
- Create: `tests/generate.rs`

- [ ] **Step 1: Create `tests/generate.rs`**

```rust
mod common;

use inkworm::ui::generate::{GenerateSubstate, PastingState};
use inkworm::ui::task_msg::{GenerateProgress, TaskMsg};

#[test]
fn pasting_state_transitions() {
    let mut state = PastingState::new();
    assert_eq!(state.byte_count(), 0);
    assert_eq!(state.word_count(), 0);
    assert!(!state.can_submit(100));

    state.text = "test article".to_string();
    assert_eq!(state.byte_count(), 12);
    assert_eq!(state.word_count(), 2);
    assert!(state.can_submit(100));

    state.text = "a".repeat(101);
    assert!(!state.can_submit(100));
}

#[test]
fn generate_progress_enum_variants() {
    // Smoke test that all variants compile
    let _p1 = GenerateProgress::Phase1Started;
    let _p2 = GenerateProgress::Phase1Done { sentence_count: 5 };
    let _p3 = GenerateProgress::Phase2Progress { done: 3, total: 5 };
}

#[tokio::test]
async fn task_msg_channel_flow() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);

    tokio::spawn(async move {
        tx.send(TaskMsg::Generate(GenerateProgress::Phase1Started)).await.unwrap();
        tx.send(TaskMsg::Generate(GenerateProgress::Phase1Done { sentence_count: 3 })).await.unwrap();
    });

    let msg1 = rx.recv().await.unwrap();
    assert!(matches!(msg1, TaskMsg::Generate(GenerateProgress::Phase1Started)));

    let msg2 = rx.recv().await.unwrap();
    assert!(matches!(msg2, TaskMsg::Generate(GenerateProgress::Phase1Done { sentence_count: 3 })));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test generate`
Expected: 3 tests PASS

- [ ] **Step 3: Commit**

```bash
git add tests/generate.rs
git commit -m "test(generate): add integration tests for Generate flow"
```

---

## Final Verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 2: Build release binary**

Run: `cargo build --release`
Expected: builds successfully

- [ ] **Step 3: Manual smoke test (optional)**

Set up test environment and verify:
1. `/import` opens Generate screen
2. Paste article (bracketed paste works)
3. Ctrl+Enter submits
4. Progress updates appear
5. Course saves and loads into Study
6. `/delete` confirms and removes course

---

## Self-Review Checklist

**Spec coverage:**
- [x] TaskMsg + GenerateProgress types (Task 1)
- [x] Error banner (Task 2)
- [x] Generate screen state (Task 3)
- [x] Reflexion progress integration (Task 4)
- [x] App state extension (Task 5)
- [x] Event loop extension (Task 6)
- [x] Main.rs wiring (Task 7)
- [x] Generate rendering (Task 8)
- [x] delete_course function (Task 9)
- [x] Integration tests (Task 10)

**Placeholder scan:**
- No TBD/TODO markers
- All code blocks complete
- All test expectations specified

**Type consistency:**
- GenerateProgress variants match across files
- TaskMsg enum consistent
- Screen enum variants match

**No gaps:** All spec requirements covered by tasks.

