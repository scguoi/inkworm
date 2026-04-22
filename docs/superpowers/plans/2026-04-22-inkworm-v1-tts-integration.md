# Plan 6d: TTS App Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the `IflytekSpeaker` shipped in Plan 6c into the running app so English drills are actually spoken as the user studies. Also add a persisted `rodio::Sink` inside `IflytekSpeaker` so `cancel()` stops playback mid-audio (Plan 6c's `mem::forget` fire-and-forget made that impossible). Device detection, wizard TTS steps, and the `/tts` status overlay are deferred to Plan 6e.

**Architecture:** `main.rs` tries to open `rodio::OutputStream::try_default()`; on success it holds the stream alive on the main thread and passes the `OutputStreamHandle` (Send+Sync) into `build_speaker`; on failure it passes `None` (cache-only mode). The speaker lands as `speaker: Arc<dyn Speaker>` on `App`. A new `App::speak_current_drill()` method cancels the previous speak and spawns a new one for the current drill's English text; it is called after every state transition that can change the active drill (`App::new`, `study.advance()`, `study.skip()`, `switch_to_course`). `IflytekSpeaker` gains `current_sink: Arc<Mutex<Option<rodio::Sink>>>` so `cancel()` can call `sink.stop()` on an in-progress playback.

**Tech Stack:** Rust · existing `rodio` 0.19 · existing `tokio-tungstenite` · `Arc<dyn Speaker>` for sharing across tokio tasks.

---

## Scope & Non-Goals

**In scope (this plan):**
- `IflytekSpeaker::current_sink` field + persistence in `play_pcm` + `Sink::stop()` in `cancel()`.
- `App::speaker: Arc<dyn Speaker>` field.
- `App::new` takes the speaker as its final parameter.
- `App::speak_current_drill()` method: cancel prior + spawn new speak for current drill's English.
- Call sites inside `App`: after `study.advance()`, after `study.skip()`, after `switch_to_course`. main.rs calls once after `App::new`.
- `main.rs`: open `rodio::OutputStream::try_default()`, degrade gracefully on error, construct speaker via `build_speaker`, pass to `App::new`.
- Update all 6 existing integration-test `make_app` helpers to pass `Arc::new(NullSpeaker)`.
- One new integration test file `tests/tts_app_wiring.rs` using a `MockSpeaker` to verify speak/cancel call sequence across drill transitions.

**Out of scope (Plan 6e):**
- Device detection (`SwitchAudioSource` / `system_profiler`), `OutputKind`, `should_speak(mode, device, creds)`.
- 1-second device-change tick in the App event loop.
- Config wizard TTS steps (app_id / api_key / api_secret entry).
- `/tts` no-args status overlay.
- 3-strikes session-disable per spec §7.6.
- `AppError::Tts` variant + `user_message` mapping.

---

## File Structure

- **Modify** `src/tts/iflytek.rs`: add `current_sink: Arc<Mutex<Option<rodio::Sink>>>` field; persist sink in `play_pcm`; stop it in `cancel`.
- **Modify** `src/app.rs`: add `speaker: Arc<dyn Speaker>` field, new param on `App::new`, new method `speak_current_drill`; inject calls after `advance`/`skip`/`switch_to_course`.
- **Modify** `src/main.rs`: construct `OutputStream` + speaker, pass to `App::new`.
- **Modify** 6 test files' `make_app` helpers to accept/default the speaker param.
- **Create** `tests/tts_app_wiring.rs`: MockSpeaker + 3 integration tests.

---

## Pre-Task Setup

- [ ] **Setup 0.1: Verify clean main and create worktree**

```bash
cd /Users/scguo/.tries/2026-04-21-scguoi-inkworm
git status
git log --oneline -3    # HEAD is ad5ee0c (Plan 6c merge)
git worktree add -b feat/v1-tts-integration ../inkworm-tts-integration main
cd ../inkworm-tts-integration
cargo test --all        # baseline 222
```

Expected: 222 tests passing.

---

## Task 1: Persisted `Sink` for mid-playback cancel

**Files:** `src/tts/iflytek.rs`

- [ ] **Step 1.1: Add the field**

Add a new field to the `IflytekSpeaker` struct:

```rust
    /// Stores the most recent rodio Sink so `cancel()` can stop mid-playback.
    /// `None` when no audio has been queued yet or when `audio` is `None`.
    current_sink: Arc<Mutex<Option<rodio::Sink>>>,
```

Initialise it in both `new` and `with_base_url`:

```rust
            current_sink: Arc::new(Mutex::new(None)),
```

- [ ] **Step 1.2: Update `play_pcm` to persist the sink**

Replace the existing `play_pcm` method body with:

```rust
    fn play_pcm(&self, samples: Vec<i16>) -> Result<(), TtsError> {
        let Some(handle) = &self.audio else {
            return Ok(());
        };
        let sink = rodio::Sink::try_new(handle)
            .map_err(|e| TtsError::Audio(e.to_string()))?;
        sink.append(rodio::buffer::SamplesBuffer::new(
            wav::CHANNELS,
            wav::SAMPLE_RATE,
            samples,
        ));
        // Store the sink so `cancel()` can stop playback mid-audio. The
        // previous sink (if any) is dropped here — `rodio::Sink` Drop does
        // NOT stop playback, it just lets the audio finish naturally. For
        // this v1 that's fine: the old audio was from a prior drill that
        // either finished already or was explicitly stopped via `cancel`.
        if let Ok(mut guard) = self.current_sink.lock() {
            *guard = Some(sink);
        }
        Ok(())
    }
```

- [ ] **Step 1.3: Update `cancel` to stop playback**

Replace the existing `cancel` method body with:

```rust
    fn cancel(&self) {
        // Cancel any in-flight WS stream.
        if let Ok(mut guard) = self.stream_handle.lock() {
            if let Some(token) = guard.take() {
                token.cancel();
            }
        }
        // Stop any currently-playing audio.
        if let Ok(mut guard) = self.current_sink.lock() {
            if let Some(sink) = guard.take() {
                sink.stop();
            }
        }
    }
```

- [ ] **Step 1.4: Run existing tests**

```bash
cd /Users/scguo/.tries/inkworm-tts-integration
cargo test --lib tts::iflytek
cargo test --test iflytek_speaker
```

Expected: all 3 unit tests + 3 integration tests still pass. The persisted-sink change is backward-compatible for cache-only mode (audio=None means no sink is ever created).

- [ ] **Step 1.5: rustfmt + clippy**

```bash
rustfmt --edition 2021 --check src/tts/iflytek.rs
cargo clippy --all-targets -- -D warnings 2>&1 | grep "tts/iflytek" | head
```

Expected: fmt silent; no NEW clippy on iflytek.rs.

- [ ] **Step 1.6: Commit**

```bash
git add src/tts/iflytek.rs
git commit -m "feat(tts): persist rodio Sink on IflytekSpeaker so cancel stops playback"
```

---

## Task 2: `App` gains a `speaker` field + `speak_current_drill` method

**Files:** `src/app.rs`

- [ ] **Step 2.1: Add imports + field + new-signature param**

At the top of `src/app.rs`, add:

```rust
use crate::tts::speaker::Speaker;
```

Inside `pub struct App { ... }`, add as the last field:

```rust
    pub speaker: Arc<dyn Speaker>,
```

Update `App::new` signature — add `speaker: Arc<dyn Speaker>` as the final parameter and initialize the field:

```rust
    pub fn new(
        course: Option<Course>,
        progress: Progress,
        data_paths: DataPaths,
        clock: Arc<dyn Clock>,
        config: Config,
        task_tx: mpsc::Sender<TaskMsg>,
        speaker: Arc<dyn Speaker>,
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
            config_wizard: None,
            course_list: None,
            speaker,
        }
    }
```

- [ ] **Step 2.2: Add `speak_current_drill` method**

Add a new public method on `impl App` (place it near `open_course_list` for grouping with other app-level helpers):

```rust
    /// Cancel any in-flight speak, then if there is a current drill,
    /// spawn a new speak for its English text. Safe to call on any state
    /// transition — no-ops cleanly when no drill is active.
    pub fn speak_current_drill(&self) {
        self.speaker.cancel();
        let Some(drill) = self.study.current_drill() else { return };
        let text = drill.english.clone();
        let speaker = Arc::clone(&self.speaker);
        tokio::spawn(async move {
            let _ = speaker.speak(&text).await;
        });
    }
```

- [ ] **Step 2.3: Call it after every drill transition**

Find `handle_study_key`. After both `self.study.advance()` (inside the `FeedbackState::Correct` arm) and `self.study.skip()` (inside the `FeedbackState::Typing` arm), add `self.speak_current_drill();`.

Here is the updated body of `handle_study_key` — replace the existing one:

```rust
    fn handle_study_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('p') => {
                    self.palette = Some(PaletteState::new());
                    self.screen = Screen::Palette;
                }
                KeyCode::Char('c') => self.quit(),
                _ => {}
            }
            return;
        }
        match self.study.feedback() {
            FeedbackState::Correct => {
                self.study.advance();
                self.speak_current_drill();
            }
            FeedbackState::Wrong { .. } => match key.code {
                KeyCode::Char(c) => self.study.type_char(c),
                KeyCode::Backspace => self.study.backspace(),
                KeyCode::Enter => self.study.submit(self.clock.as_ref()),
                _ => {}
            },
            FeedbackState::Typing => match key.code {
                KeyCode::Char(c) => self.study.type_char(c),
                KeyCode::Backspace => self.study.backspace(),
                KeyCode::Enter => self.study.submit(self.clock.as_ref()),
                KeyCode::Tab => {
                    self.study.skip();
                    self.speak_current_drill();
                }
                _ => {}
            },
        }
    }
```

Then find `execute_command`'s `"skip" => self.study.skip(),` arm and change it to:

```rust
            "skip" => {
                self.study.skip();
                self.speak_current_drill();
            }
```

Find `switch_to_course`. At the very end of the success path (after `self.screen = Screen::Study;`), add `self.speak_current_drill();`. Here is the expected tail of the function:

```rust
        self.study = crate::ui::study::StudyState::new(Some(course), progress);
        self.course_list = None;
        self.screen = Screen::Study;
        self.speak_current_drill();
    }
```

Find `handle_generate_progress`'s `GenerateProgress::Done(course)` arm — after the block completes with `self.screen = Screen::Study;`, add a `self.speak_current_drill();` at the end of that arm (we just loaded a new course, the first drill is about to be shown).

```rust
            GenerateProgress::Done(course) => {
                // ... existing code up through ...
                self.study = StudyState::new(Some(course), self.study.progress().clone());
                self.generate = None;
                self.screen = Screen::Study;
                self.speak_current_drill();
            }
```

- [ ] **Step 2.4: Compile check — expect `App::new` call sites to fail**

```bash
cargo check 2>&1 | tail -30
```

Expected: `main.rs` AND every test helper that calls `App::new` fails to compile because it's missing the new final `speaker` argument. Task 3 fixes main.rs; Task 4 fixes the tests. You are NOT fixing them in this task.

- [ ] **Step 2.5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add speaker field and speak_current_drill drill-transition hook"
```

(This commit leaves the crate in a broken-compile state. That's OK — Tasks 3 + 4 fix it in the next two commits. The branch will not ship until all three tasks are in.)

---

## Task 3: `main.rs` constructs the speaker

**Files:** `src/main.rs`

- [ ] **Step 3.1: Replace `main.rs` body**

Current `main.rs` builds `App::new(course, progress, paths, Arc::new(SystemClock), config, task_tx)`. Update it to also construct an `rodio::OutputStream` and a speaker. Replace the file with:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use inkworm::app::App;
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::load_course;
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::tts::speaker::build_speaker;
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
        Ok(c) => {
            for err in c.validate_llm() {
                eprintln!("config: {err}");
            }
            (c, true)
        }
        Err(e) => {
            eprintln!("config: could not load {:?}: {e}", paths.config_file);
            (Config::default(), true)
        }
    };

    let progress = Progress::load(&paths.progress_file)?;

    let course = progress
        .active_course_id
        .as_deref()
        .and_then(|id| load_course(&paths.courses_dir, id).ok());

    // Try to open a rodio OutputStream once, up-front. `OutputStream` itself
    // is `!Send` and must stay alive on this (main) thread for audio to
    // continue playing. We pass its `OutputStreamHandle` (Send+Sync) into
    // the speaker. On failure we fall back to a silent speaker — the user
    // can still warm the cache via /tts on, but playback is disabled.
    let (_output_stream, audio_handle) = match rodio::OutputStream::try_default() {
        Ok((stream, handle)) => (Some(stream), Some(handle)),
        Err(e) => {
            eprintln!("TTS: audio device unavailable ({e}). Playback disabled.");
            (None, None)
        }
    };

    let speaker: Arc<dyn inkworm::tts::speaker::Speaker> = Arc::from(build_speaker(
        &config.tts.iflytek,
        paths.tts_cache_dir.clone(),
        config.tts.r#override,
        audio_handle,
    ));

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
            speaker,
        );
        if needs_wizard {
            app.open_wizard(WizardOrigin::FirstRun);
        }
        // Speak the current drill on startup (no-op if no course loaded).
        app.speak_current_drill();
        run_loop(&mut guard, &mut app, task_rx).await
    })?;

    Ok(())
}
```

- [ ] **Step 3.2: Verify main compiles (tests will still fail)**

```bash
cargo check 2>&1 | grep -E "error\[|error:" | head
```

Expected: `error` count drops significantly. There will still be errors in the `tests/` crates (Task 4 fixes those).

- [ ] **Step 3.3: Commit**

```bash
git add src/main.rs
git commit -m "feat(main): construct rodio OutputStream + speaker and pass to App"
```

---

## Task 4: Update all integration-test `make_app` helpers

**Files:**
- `tests/ui.rs`
- `tests/generate.rs`
- `tests/config_wizard.rs`
- `tests/course_list.rs`
- `tests/tts_palette.rs`

- [ ] **Step 4.1: Inventory the helpers**

Find every `App::new(...)` call in `tests/`:

```bash
cd /Users/scguo/.tries/inkworm-tts-integration
grep -rn "App::new" tests/
```

Each caller needs a final `speaker` arg. These tests don't care about audio — they pass `Arc::new(NullSpeaker)`.

- [ ] **Step 4.2: Update each test file**

For each of the 5 test files, at the top of the helpers section, add:

```rust
use std::sync::Arc;
use inkworm::tts::speaker::{NullSpeaker, Speaker};
```

(If `std::sync::Arc` is already imported, skip that line. Same for `Speaker` — the import is only needed for the type annotation; if the file prefers a trait object via concrete type it may not need the trait.)

Then in each `make_app` helper, add a `speaker: Arc<dyn Speaker> = Arc::new(NullSpeaker);` binding and pass it as the final argument. Example replacement for `tests/tts_palette.rs`:

```rust
fn make_app(paths: DataPaths) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    let speaker: Arc<dyn Speaker> = Arc::new(NullSpeaker);
    App::new(
        None,
        Progress::empty(),
        paths,
        Arc::new(SystemClock),
        Config::default(),
        task_tx,
        speaker,
    )
}
```

Apply the same surgical shape to `make_app` in:
- `tests/ui.rs` — may have multiple helpers; update each
- `tests/generate.rs` — check for all `App::new` call sites
- `tests/config_wizard.rs` — same
- `tests/course_list.rs` — same

**If a test file has multiple `App::new` call sites**, extract a single helper if one doesn't exist. Otherwise update each call in place.

- [ ] **Step 4.3: Run the full suite**

```bash
cargo test --all
```

Expected: 222 tests still pass (no net change — we only added the parameter; behavior is unchanged because `NullSpeaker::speak` is always `Ok(())` and `cancel` is a no-op).

- [ ] **Step 4.4: rustfmt check on touched files**

```bash
rustfmt --edition 2021 --check tests/ui.rs tests/generate.rs tests/config_wizard.rs tests/course_list.rs tests/tts_palette.rs
```

Expected: silent.

- [ ] **Step 4.5: Commit**

```bash
git add tests/ui.rs tests/generate.rs tests/config_wizard.rs tests/course_list.rs tests/tts_palette.rs
git commit -m "test: pass NullSpeaker to App::new in all integration test helpers"
```

---

## Task 5: Integration test — verify speaker called on drill transitions

**Files:** `tests/tts_app_wiring.rs`

- [ ] **Step 5.1: Create `tests/tts_app_wiring.rs`**

The test uses a `MockSpeaker` that records every `speak` call and `cancel` call. It then drives an App through typing a correct drill, hitting any key to advance, and asserts the mock saw `speak("english text of new drill")`.

```rust
//! Integration tests for App ↔ Speaker wiring.
//!
//! Uses a `MockSpeaker` that records speak/cancel invocations so we can
//! assert the right English text was spoken at the right drill transition.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use inkworm::app::App;
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::{load_course, save_course, Course};
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::tts::speaker::{Speaker, TtsError};
use tokio::sync::mpsc;

struct MockSpeaker {
    spoken: Arc<Mutex<Vec<String>>>,
    cancels: Arc<AtomicUsize>,
}

impl MockSpeaker {
    fn new() -> (Arc<Self>, Arc<Mutex<Vec<String>>>, Arc<AtomicUsize>) {
        let spoken = Arc::new(Mutex::new(Vec::<String>::new()));
        let cancels = Arc::new(AtomicUsize::new(0));
        let mock = Arc::new(Self {
            spoken: Arc::clone(&spoken),
            cancels: Arc::clone(&cancels),
        });
        (mock, spoken, cancels)
    }
}

#[async_trait]
impl Speaker for MockSpeaker {
    async fn speak(&self, text: &str) -> Result<(), TtsError> {
        self.spoken.lock().unwrap().push(text.to_string());
        Ok(())
    }
    fn cancel(&self) {
        self.cancels.fetch_add(1, Ordering::SeqCst);
    }
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn seed_one_course(paths: &DataPaths) -> Course {
    let base = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    let course: Course = serde_json::from_str(&base).unwrap();
    save_course(&paths.courses_dir, &course).unwrap();
    course
}

fn make_app(paths: DataPaths, speaker: Arc<dyn Speaker>, course: Option<Course>) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    let mut progress = Progress::empty();
    if let Some(c) = &course {
        progress.active_course_id = Some(c.id.clone());
    }
    App::new(
        course,
        progress,
        paths,
        Arc::new(SystemClock),
        Config::default(),
        task_tx,
        speaker,
    )
}

async fn settle() {
    // speak_current_drill spawns a tokio task; yield so it runs.
    tokio::time::sleep(Duration::from_millis(20)).await;
}

#[tokio::test]
async fn skip_advances_drill_and_speaks_new_english() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_one_course(&paths);
    let (mock, spoken, cancels) = MockSpeaker::new();
    let mut app = make_app(paths, mock.clone(), Some(course.clone()));

    // Startup speak fires once from the initial state.
    app.speak_current_drill();
    settle().await;

    let before_count = spoken.lock().unwrap().len();

    // Tab to skip the current drill → should cancel previous + speak the next drill.
    app.on_input(key(KeyCode::Tab));
    settle().await;

    assert!(cancels.load(Ordering::SeqCst) >= 2, "at least two cancels: startup + skip");
    let spoken_snapshot = spoken.lock().unwrap().clone();
    assert!(
        spoken_snapshot.len() > before_count,
        "speak was invoked after skip, got {spoken_snapshot:?}"
    );
    // The new drill's English should be the one spoken last.
    let expected = course.sentences[0].drills[1].english.clone();
    assert_eq!(spoken_snapshot.last().unwrap(), &expected);
}

#[tokio::test]
async fn correct_answer_then_any_key_advances_and_speaks() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_one_course(&paths);
    let (mock, spoken, _cancels) = MockSpeaker::new();
    let mut app = make_app(paths, mock.clone(), Some(course.clone()));

    // Type the exact first-drill english, then Enter (→ Correct), then any key (→ advance).
    let first_drill_english = course.sentences[0].drills[0].english.clone();
    for c in first_drill_english.chars() {
        app.on_input(key(KeyCode::Char(c)));
    }
    app.on_input(key(KeyCode::Enter));
    // After Enter, feedback is Correct; next key press triggers advance().
    app.on_input(key(KeyCode::Char(' ')));
    settle().await;

    let spoken_snapshot = spoken.lock().unwrap().clone();
    let next_english = course.sentences[0].drills[1].english.clone();
    assert!(
        spoken_snapshot.iter().any(|s| s == &next_english),
        "expected to have spoken {:?}, got {:?}",
        next_english,
        spoken_snapshot,
    );
}

#[tokio::test]
async fn switch_to_course_speaks_new_course_first_drill() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    // Seed two courses.
    let base = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    let mut v1: serde_json::Value = serde_json::from_str(&base).unwrap();
    v1["id"] = serde_json::Value::String("course-a".into());
    let course_a: Course = serde_json::from_value(v1).unwrap();
    save_course(&paths.courses_dir, &course_a).unwrap();

    let mut v2: serde_json::Value = serde_json::from_str(&base).unwrap();
    v2["id"] = serde_json::Value::String("course-b".into());
    v2["sentences"][0]["drills"][0]["english"] =
        serde_json::Value::String("Hello other course".into());
    let course_b: Course = serde_json::from_value(v2).unwrap();
    save_course(&paths.courses_dir, &course_b).unwrap();

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let mut app = make_app(paths.clone(), mock.clone(), Some(course_a.clone()));

    // Open list, select course-b (index 1 after sort, since both share createdAt;
    // seed_one_course gives them the same timestamp, so order is filesystem-defined —
    // just find the item whose id is course-b).
    app.open_course_list();
    let list = app.course_list.as_ref().unwrap();
    let target_idx = list.items.iter().position(|i| i.meta.id == "course-b").unwrap();
    while app.course_list.as_ref().unwrap().selected != target_idx {
        app.on_input(key(KeyCode::Down));
    }
    app.on_input(key(KeyCode::Enter));
    settle().await;

    let spoken_snapshot = spoken.lock().unwrap().clone();
    assert!(
        spoken_snapshot.iter().any(|s| s == "Hello other course"),
        "expected course-b first drill to have been spoken, got {spoken_snapshot:?}"
    );
    // Sanity: the load_course result should be course-b now.
    let reloaded = load_course(&paths.courses_dir, "course-b").unwrap();
    assert_eq!(reloaded.id, "course-b");
}
```

- [ ] **Step 5.2: Run the test**

```bash
cargo test --test tts_app_wiring
```

Expected: 3 tests pass.

- [ ] **Step 5.3: Full suite + rustfmt**

```bash
cargo test --all 2>&1 | grep "test result" | awk '{sum+=$4} END {print "total:", sum}'
rustfmt --edition 2021 --check tests/tts_app_wiring.rs
```

Expected: total 225 (222 + 3); rustfmt silent.

- [ ] **Step 5.4: Commit**

```bash
git add tests/tts_app_wiring.rs
git commit -m "test(tts): integration tests for app-level speak/cancel on drill transitions"
```

---

## Task 6: Doc sync + session log + PR

**Files:**
- Maybe-modify: `docs/superpowers/specs/2026-04-21-inkworm-design.md`
- Create: `docs/superpowers/progress/2026-04-22-plan-6d-session-log.md`

- [ ] **Step 6.1: Spec divergence check**

§7.2 spec claims cancel stops WS and sink. Now it does. If §7.2 matches the implementation, no sync needed. If divergent, add a `docs: sync ...` commit.

- [ ] **Step 6.2: Write session log**

Create `docs/superpowers/progress/2026-04-22-plan-6d-session-log.md`. Summarize:
- Goals, commits (5 task commits + session log)
- Test counts (baseline 222 → final 225)
- Deviations if any
- Follow-ups for Plan 6e: device detect, wizard TTS steps, `/tts` status overlay, 3-strikes session disable, `AppError::Tts` + `user_message`
- Manual smoke test checklist (requires real iFlytek creds): `/tts on`, type a drill, expect audio

- [ ] **Step 6.3: Final verification**

```bash
rustfmt --edition 2021 --check $(git diff --name-only main..HEAD | grep '\.rs$')
cargo clippy --all-targets -- -D warnings 2>&1 | grep -cE "^error:"   # compare to pre-existing baseline
cargo test --all
git status    # clean
```

Expected: rustfmt silent on touched files; clippy unchanged from baseline; all 225 tests pass.

- [ ] **Step 6.4: Commit session log**

```bash
git add docs/superpowers/progress/2026-04-22-plan-6d-session-log.md
git commit -m "docs: add session log for Plan 6d completion"
```

- [ ] **Step 6.5: Push and open PR**

```bash
git push -u origin feat/v1-tts-integration
gh pr create --title "Plan 6d: TTS App integration (live speaker + persisted Sink)" --body "$(cat <<'EOF'
## Summary
- \`IflytekSpeaker\` now persists the active \`rodio::Sink\` so \`cancel()\` stops playback mid-audio (Plan 6c used \`mem::forget\`)
- \`App\` gains \`speaker: Arc<dyn Speaker>\` and a new \`speak_current_drill()\` method that cancels prior + spawns new speak for the current drill's English
- Drill transitions wire into \`speak_current_drill()\`: Tab skip, correct-answer-any-key advance, \`/skip\` palette, course switch, /import done, App startup
- \`main.rs\` constructs \`rodio::OutputStream\` up-front; degrades gracefully when no audio device is available (speaker runs in cache-only mode)
- MockSpeaker-based integration tests verify speak/cancel call sequence across transitions

## Non-Goals (deferred to Plan 6e)
- Device detection (\`SwitchAudioSource\` / \`system_profiler\`), \`should_speak(mode, device, creds)\`
- Config wizard TTS steps (app_id / api_key / api_secret)
- \`/tts\` no-args status overlay
- 3-strikes session-disable per spec §7.6
- \`AppError::Tts\` variant + user_message mapping

## Test plan
- [x] cargo test --all — 225 passing (222 baseline + 3 new integration)
- [x] rustfmt --check on touched files — clean
- [x] No new clippy warnings introduced
- [ ] Manual smoke: supply real iFlytek creds via /config (Plan 6e will add wizard steps; today you'd edit config.toml directly), /tts on, type a drill, confirm TTS plays
- [ ] Manual smoke: /tts off, confirm no TTS fires

See \`docs/superpowers/progress/2026-04-22-plan-6d-session-log.md\` for the full per-task breakdown.
EOF
)"
```

---

## Self-Review Checklist

- **Spec coverage:**
  - §7.1 cache-hit + cache-miss flow → carried through from Plan 6c; now integrated into App ✓
  - §7.2 cancel semantics (WS close + sink stop) → Task 1 (sink) + Plan 6c (WS) ✓
  - §7.3 audio format — inherited ✓
  - §7.6 error degradation: rodio failure → NullSpeaker — Task 3 falls back to cache-only, not full Null. Acceptable given IflytekSpeaker+None is equivalent to Null for user-visible behavior.
- **Placeholder scan:** every code block is complete; no "TBD" / "similar to".
- **Type consistency:**
  - `App::new(course, progress, paths, clock, config, task_tx, speaker)` — defined in Task 2, used in Task 3 (main), Task 4 (tests), Task 5 (tests) ✓
  - `App::speak_current_drill(&self)` — Task 2 + called in Task 3 (main) + Tasks 2+5 call sites ✓
  - `Arc<dyn Speaker>` type — consistent across Tasks 2/3/4/5 ✓
  - `MockSpeaker` — Task 5 local; its `Speaker` impl matches the trait ✓
- **Broken-build commit (Task 2)** — explicitly called out; fixed by Tasks 3 + 4 in subsequent commits. Branch integrity restored before any PR is opened.

---

## Execution Handoff

**Plan complete.** Default = Subagent-Driven.
