# Bundled Course Audio Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow each course to ship pre-generated `.mp3` audio in a sibling directory; play it directly when a drill is studied; fall through to existing iFlytek TTS when missing.

**Architecture:** New `src/audio/` module with `bundle` (path resolution) and `player` (mp3 decode + rodio playback). `App` resolves bundle path before each `speaker.speak()` call; bundle hits play directly via a separate rodio `Sink`, bundle misses fall through to existing TTS path unchanged.

**Tech Stack:** Rust, `rodio` 0.19 with `mp3` feature (minimp3 decoder), `tokio` `spawn_blocking` for sync decode, existing `OutputStreamHandle`-sharing pattern.

**Spec:** `docs/superpowers/specs/2026-05-07-bundled-course-audio-design.md`

---

## File Structure

**New files:**
- `src/audio/mod.rs` — module entry + re-exports
- `src/audio/bundle.rs` — `bundle_path()` + `bundle_exists()` (pure path math + stat)
- `src/audio/player.rs` — `BundlePlayer` (rodio `Sink` owner + cancel + mp3 decode)
- `tests/bundled_audio.rs` — integration tests
- `fixtures/audio/silence.mp3` — committed minimal mp3 fixture (~100 bytes)

**Modified files:**
- `Cargo.toml` — add `mp3` feature to rodio
- `src/lib.rs` — register `audio` module
- `src/ui/study.rs` — add `current_sentence()` helper
- `src/app.rs` — add `bundle_player` field; refactor `speak_current_drill`
- `src/main.rs` — construct `BundlePlayer` + pass into `App::new`
- `tests/tts_app_wiring.rs` — `make_app()` helper updated for new `App::new` arg

---

## Task 1: Bootstrap audio module + Cargo feature

**Files:**
- Modify: `Cargo.toml`
- Create: `src/audio/mod.rs`
- Create: `src/audio/bundle.rs`
- Create: `src/audio/player.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add `mp3` feature to rodio**

Edit `Cargo.toml`, locate the line:
```toml
rodio = { version = "0.19", default-features = false }
```
Replace with:
```toml
rodio = { version = "0.19", default-features = false, features = ["mp3"] }
```

- [ ] **Step 2: Create empty audio module skeleton**

Create `src/audio/mod.rs`:
```rust
//! Bundled course audio: path resolution + mp3 playback.
//!
//! Independent of `crate::tts` — bundles never sign requests, never hit
//! the network, and never share the iFlytek wav cache. See
//! `docs/superpowers/specs/2026-05-07-bundled-course-audio-design.md`.

pub mod bundle;
pub mod player;
```

Create `src/audio/bundle.rs`:
```rust
//! Path resolution for course-bundled audio files.
```

Create `src/audio/player.rs`:
```rust
//! Mp3 playback for course-bundled audio.
```

- [ ] **Step 3: Register module in lib.rs**

Edit `src/lib.rs`. Add `pub mod audio;` after `pub mod app;`:
```rust
pub mod app;
pub mod audio;
pub mod clock;
// ...rest unchanged
```

- [ ] **Step 4: Verify build**

Run: `cargo build`
Expected: success. The `mp3` feature pulls in `minimp3` transitively; first build will recompile rodio.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/audio/
git commit -m "feat(audio): bootstrap audio module with mp3 feature"
```

---

## Task 2: `bundle_path()` and `bundle_exists()` (TDD)

**Files:**
- Modify: `src/audio/bundle.rs`

- [ ] **Step 1: Write the failing tests**

Replace the contents of `src/audio/bundle.rs` with:

```rust
//! Path resolution for course-bundled audio files.
//!
//! Layout (per spec §2):
//! `<courses_dir>/<yyyy-mm>/<id_tail>/s{order:02}-d{stage}.mp3`
//! where `id_tail` is everything after `yyyy-mm-dd-` (i.e. `id[8..]`).

use std::path::{Path, PathBuf};

use crate::storage::course::StorageError;

/// Resolve the on-disk path for a single drill's bundled mp3.
///
/// Returns `StorageError::InvalidId` when `course_id` does not begin
/// with the `yyyy-mm-dd-` prefix. Does NOT check whether the file
/// exists — use `bundle_exists` for that.
pub fn bundle_path(
    courses_dir: &Path,
    course_id: &str,
    order: u32,
    stage: u32,
) -> Result<PathBuf, StorageError> {
    if !has_yyyy_mm_dd_prefix(course_id) {
        return Err(StorageError::InvalidId(course_id.to_string()));
    }
    let yyyy_mm = &course_id[0..7]; // "2026-05"
    let id_tail = &course_id[8..]; // "06-foo-bar"
    let file = format!("s{:02}-d{}.mp3", order, stage);
    Ok(courses_dir.join(yyyy_mm).join(id_tail).join(file))
}

/// Convenience: returns `true` iff `bundle_path` resolves AND the file
/// exists. Any error (invalid id, IO error) maps to `false`.
pub fn bundle_exists(courses_dir: &Path, course_id: &str, order: u32, stage: u32) -> bool {
    match bundle_path(courses_dir, course_id, order, stage) {
        Ok(p) => p.is_file(),
        Err(_) => false,
    }
}

fn has_yyyy_mm_dd_prefix(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 11
        && b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && b[10] == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_path_yyyy_mm_split() {
        let p =
            bundle_path(Path::new("/tmp/courses"), "2026-05-06-foo", 1, 1).unwrap();
        assert_eq!(
            p,
            PathBuf::from("/tmp/courses/2026-05/06-foo/s01-d1.mp3")
        );
    }

    #[test]
    fn bundle_path_pads_order_to_two_digits() {
        let p =
            bundle_path(Path::new("/c"), "2026-05-06-x", 9, 1).unwrap();
        assert!(p.ends_with("s09-d1.mp3"), "got {p:?}");
        let p =
            bundle_path(Path::new("/c"), "2026-05-06-x", 12, 3).unwrap();
        assert!(p.ends_with("s12-d3.mp3"), "got {p:?}");
    }

    #[test]
    fn bundle_path_invalid_id_errors() {
        let err = bundle_path(Path::new("/c"), "no-prefix", 1, 1).unwrap_err();
        assert!(matches!(err, StorageError::InvalidId(_)), "got {err:?}");
    }

    #[test]
    fn bundle_exists_false_when_dir_missing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!bundle_exists(tmp.path(), "2026-05-06-foo", 1, 1));
    }

    #[test]
    fn bundle_exists_true_when_file_present() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("2026-05").join("06-foo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s01-d1.mp3"), b"").unwrap();
        assert!(bundle_exists(tmp.path(), "2026-05-06-foo", 1, 1));
    }

    #[test]
    fn bundle_exists_false_for_other_stage() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("2026-05").join("06-foo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s01-d1.mp3"), b"").unwrap();
        assert!(!bundle_exists(tmp.path(), "2026-05-06-foo", 1, 2));
    }

    #[test]
    fn bundle_exists_false_for_invalid_id() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!bundle_exists(tmp.path(), "no-prefix", 1, 1));
    }
}
```

- [ ] **Step 2: Run tests — they should pass on first run**

Run: `cargo test -p inkworm --lib audio::bundle::tests`
Expected: 6 tests pass.

If they fail, the implementation in Step 1 is wrong — fix until green. (This is a "tests + impl together" task because the path math is short enough that a separate red phase is wasteful.)

- [ ] **Step 3: Run full test suite to confirm no regression**

Run: `cargo test`
Expected: all existing tests still pass; 6 new pass.

- [ ] **Step 4: Commit**

```bash
git add src/audio/bundle.rs
git commit -m "feat(audio): bundle path resolution and existence check"
```

---

## Task 3: Generate `silence.mp3` fixture

**Files:**
- Create: `fixtures/audio/silence.mp3`

- [ ] **Step 1: Verify ffmpeg is available**

Run: `which ffmpeg`
Expected: a path. If missing on macOS, `brew install ffmpeg` first. (Confirmed available at `/opt/homebrew/bin/ffmpeg` on the dev machine.)

- [ ] **Step 2: Generate the fixture**

Run:
```bash
mkdir -p fixtures/audio
ffmpeg -y -f lavfi -i anullsrc=r=16000:cl=mono -t 0.1 -c:a libmp3lame -b:a 16k fixtures/audio/silence.mp3
```

Expected: `fixtures/audio/silence.mp3` exists; size between 200 and 1000 bytes.

- [ ] **Step 3: Verify round-trip with rodio**

Create a throwaway test (do not commit this file — just sanity check) or skip and let Task 5's tests catch any issue.

```bash
ls -la fixtures/audio/silence.mp3
```
Expected: file size > 0.

- [ ] **Step 4: Commit**

```bash
git add fixtures/audio/silence.mp3
git commit -m "test(audio): add minimal silence.mp3 fixture for decoder tests"
```

---

## Task 4: `BundlePlayer::new` + `play()` with `audio=None` (TDD)

**Files:**
- Modify: `src/audio/player.rs`

- [ ] **Step 1: Write failing tests**

Replace contents of `src/audio/player.rs`:

```rust
//! Mp3 playback for course-bundled audio.
//!
//! `BundlePlayer` owns a single rodio `Sink` slot for the most recently
//! triggered bundle playback. Calling `play()` while audio is already
//! playing supersedes the previous sink (matches the IflytekSpeaker
//! convention). `cancel()` stops the active sink.

use std::path::Path;
use std::sync::{Arc, Mutex};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(String),
    #[error("audio: {0}")]
    Audio(String),
}

pub struct BundlePlayer {
    audio: Option<rodio::OutputStreamHandle>,
    current_sink: Arc<Mutex<Option<rodio::Sink>>>,
}

impl BundlePlayer {
    pub fn new(audio: Option<rodio::OutputStreamHandle>) -> Self {
        Self {
            audio,
            current_sink: Arc::new(Mutex::new(None)),
        }
    }

    /// Decode the mp3 at `path` and start playback. With `audio=None`
    /// (cache-only / headless mode used by tests) decode still runs so
    /// errors are observable, but no Sink is created.
    ///
    /// Replaces any prior sink. Decode runs on a blocking thread so the
    /// async caller is not stalled.
    pub async fn play(&self, path: &Path) -> Result<(), BundleError> {
        let path_owned = path.to_path_buf();
        let decoded: Result<rodio::Decoder<std::io::BufReader<std::fs::File>>, BundleError> =
            tokio::task::spawn_blocking(move || {
                let file = std::fs::File::open(&path_owned)?;
                let reader = std::io::BufReader::new(file);
                rodio::Decoder::new(reader)
                    .map_err(|e| BundleError::Decode(format!("{e}")))
            })
            .await
            .map_err(|e| BundleError::Audio(format!("join: {e}")))?;

        let source = decoded?;

        let Some(handle) = &self.audio else {
            // Cache-only mode: decode succeeded, drop the source.
            return Ok(());
        };
        let sink = rodio::Sink::try_new(handle)
            .map_err(|e| BundleError::Audio(e.to_string()))?;
        sink.append(source);
        if let Ok(mut guard) = self.current_sink.lock() {
            *guard = Some(sink);
        }
        Ok(())
    }

    /// Stop any currently-playing sink. Safe when nothing is playing.
    pub fn cancel(&self) {
        if let Ok(mut guard) = self.current_sink.lock() {
            if let Some(sink) = guard.take() {
                sink.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn play_with_no_audio_handle_decodes_real_mp3_fixture() {
        let player = BundlePlayer::new(None);
        let res = player.play(Path::new("fixtures/audio/silence.mp3")).await;
        assert!(res.is_ok(), "expected real fixture to decode, got {res:?}");
    }

    #[tokio::test]
    async fn play_missing_file_returns_io_error() {
        let player = BundlePlayer::new(None);
        let err = player
            .play(Path::new("/definitely/does/not/exist.mp3"))
            .await
            .unwrap_err();
        assert!(matches!(err, BundleError::Io(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn play_corrupt_file_returns_decode_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.mp3");
        // Random non-mp3 bytes; minimp3 should reject.
        std::fs::write(&path, b"not an mp3 at all, just text").unwrap();
        let player = BundlePlayer::new(None);
        let err = player.play(&path).await.unwrap_err();
        assert!(
            matches!(err, BundleError::Decode(_)),
            "expected Decode, got {err:?}"
        );
    }

    #[tokio::test]
    async fn play_zero_byte_file_returns_decode_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.mp3");
        std::fs::write(&path, b"").unwrap();
        let player = BundlePlayer::new(None);
        let err = player.play(&path).await.unwrap_err();
        assert!(
            matches!(err, BundleError::Decode(_)),
            "expected Decode, got {err:?}"
        );
    }

    #[test]
    fn cancel_without_active_play_is_noop() {
        let player = BundlePlayer::new(None);
        player.cancel(); // must not panic
    }
}
```

- [ ] **Step 2: Run tests — they should pass on first run**

Run: `cargo test -p inkworm --lib audio::player::tests`
Expected: 5 tests pass.

(If `play_corrupt_file_returns_decode_error` fails because minimp3 silently accepts garbage, swap the assert to allow either `Decode` or `Io`. Verify which behavior actually occurs and pin it down.)

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src/audio/player.rs
git commit -m "feat(audio): BundlePlayer with mp3 decode + cancel"
```

---

## Task 5: Add `current_sentence()` helper to `StudyState`

**Files:**
- Modify: `src/ui/study.rs`

- [ ] **Step 1: Write the failing test**

Open `src/ui/study.rs`. Locate `#[cfg(test)] mod tests { ... }` (starts at line 510). Inside this module, the existing helper is `fn fixture_course() -> Course` (line 517). Add two new tests inside this same mod (place them after `starts_at_first_drill`):

```rust
    #[test]
    fn current_sentence_returns_active_sentence() {
        let course = fixture_course();
        let state = StudyState::new(Some(course.clone()), Progress::empty());
        let s = state.current_sentence().expect("should have a sentence");
        assert_eq!(s.order, course.sentences[0].order);
    }

    #[test]
    fn current_sentence_none_when_no_course() {
        let state = StudyState::new(None, Progress::empty());
        assert!(state.current_sentence().is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkworm --lib ui::study::tests::current_sentence_returns_active_sentence`
Expected: FAIL with `no method named current_sentence`.

- [ ] **Step 3: Implement**

In `src/ui/study.rs`, locate `pub fn current_drill(&self) -> Option<&Drill>` (around line 121). Add directly below it:

```rust
    pub fn current_sentence(&self) -> Option<&Sentence> {
        self.course.as_ref()?.sentences.get(self.sentence_idx)
    }
```

If `Sentence` is not already in scope inside this file, find the existing `use crate::storage::course::{...}` line and add `Sentence` to the list. (It's likely already imported since `current_drill` references `Drill` from the same module.)

- [ ] **Step 4: Run tests to verify**

Run: `cargo test -p inkworm --lib ui::study::tests::current_sentence`
Expected: both new tests PASS.

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/ui/study.rs
git commit -m "feat(study): add current_sentence helper"
```

---

## Task 6: Add `bundle_player` field to `App` and wire from `main.rs`

**Files:**
- Modify: `src/app.rs`
- Modify: `src/main.rs`
- Modify: `tests/tts_app_wiring.rs`

This is a wiring-only task with **no behavior change**: the player is constructed and stored but never invoked. Existing tests must continue to pass.

- [ ] **Step 1: Add `bundle_player` to `App` struct**

In `src/app.rs`, find the `pub struct App { ... }` block. Locate the `pub speaker: Arc<dyn Speaker>` field. Add directly below it:

```rust
    pub bundle_player: Arc<crate::audio::player::BundlePlayer>,
```

- [ ] **Step 2: Add parameter to `App::new`**

In `src/app.rs`, find `pub fn new(...) -> Self` (line 66). Add a new parameter as the **last** argument:

```rust
        bundle_player: Arc<crate::audio::player::BundlePlayer>,
```

Inside the constructor body, locate the field initialization block (`let mut app = Self { ... }` around line 77). Add the field next to `speaker`:

```rust
            speaker,
            bundle_player,
            current_device: OutputKind::Unknown,
```

- [ ] **Step 3: Update `src/main.rs`**

In `src/main.rs`, locate the speaker construction (around line 151):

```rust
    let speaker: Arc<dyn inkworm::tts::speaker::Speaker> = Arc::from(build_speaker(
        &config.tts.iflytek,
        paths.tts_cache_dir.clone(),
        config.tts.r#override,
        audio_handle,
    ));
```

The `audio_handle` is consumed by `build_speaker`. We need to clone it before passing in. Replace that block with:

```rust
    let speaker: Arc<dyn inkworm::tts::speaker::Speaker> = Arc::from(build_speaker(
        &config.tts.iflytek,
        paths.tts_cache_dir.clone(),
        config.tts.r#override,
        audio_handle.clone(),
    ));
    let bundle_player = Arc::new(inkworm::audio::player::BundlePlayer::new(audio_handle));
```

Then in the `App::new(...)` call (around line 170), append `bundle_player` as the final argument:

```rust
        let mut app = App::new(
            course,
            progress,
            paths,
            Arc::new(SystemClock),
            config,
            mistakes,
            combined_boot_warning,
            task_tx,
            speaker,
            bundle_player,
        );
```

- [ ] **Step 4: Update `tests/tts_app_wiring.rs::make_app`**

In `tests/tts_app_wiring.rs`, find `fn make_app(...) -> App` (around line 62). Inside its body, just before the `App::new(...)` call, add:

```rust
    let bundle_player = std::sync::Arc::new(inkworm::audio::player::BundlePlayer::new(None));
```

Append `bundle_player` as the final argument to `App::new(...)`:

```rust
    App::new(
        course,
        progress,
        paths,
        Arc::new(SystemClock),
        config,
        inkworm::storage::mistakes::MistakeBook::empty(),
        None,
        task_tx,
        speaker,
        bundle_player,
    )
```

- [ ] **Step 5: Build to find any other call sites**

Run: `cargo build --all-targets 2>&1 | grep -E "error|App::new"`
Expected: no errors. If any other test file constructs `App::new`, the compiler will flag it; update each one with the same `bundle_player` arg.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: all pre-existing tests pass unchanged. No new tests in this task.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/main.rs tests/tts_app_wiring.rs
git commit -m "feat(app): wire BundlePlayer (no behavior change)"
```

---

## Task 7: Refactor `speak_current_drill` to extract `speak_via_tts` (pure refactor)

**Files:**
- Modify: `src/app.rs`

No behavior change. Extracts the spawn block into a private method so Task 8 can call it from the fall-through branch.

- [ ] **Step 1: Locate the existing function**

Read `src/app.rs:256-311` (the `speak_current_drill` method). It looks like:

```rust
    pub fn speak_current_drill(&self) {
        tracing::debug!("speak_current_drill called");
        self.speaker.cancel();
        if self.tts_session_disabled { /* skip */ return; }
        if /* Complete */ { return; }
        let Some(drill) = self.study.current_drill() else { return; };
        let should = should_speak(...);
        if !should { return; }
        let text = drill.english.clone();
        tracing::info!(...);
        let speaker = Arc::clone(&self.speaker);
        let last_error = Arc::clone(&self.last_tts_error);
        let task_tx = self.task_tx.clone();
        tokio::spawn(async move { /* speaker.speak(&text).await */ });
    }
```

- [ ] **Step 2: Extract the spawn block**

Add a new private method directly below `speak_current_drill`:

```rust
    fn speak_via_tts(&self, text: String) {
        tracing::info!("Spawning TTS task for text: {}", text);
        let speaker = Arc::clone(&self.speaker);
        let last_error = Arc::clone(&self.last_tts_error);
        let task_tx = self.task_tx.clone();
        tokio::spawn(async move {
            let result = speaker.speak(&text).await;
            match result {
                Ok(()) => {
                    *last_error.lock().await = None;
                    let _ = task_tx.send(TaskMsg::TtsSpeakResult(Ok(()))).await;
                }
                Err(e) => {
                    let is_auth = matches!(e, crate::tts::speaker::TtsError::Auth(_));
                    let message = format!("{}", e);
                    *last_error.lock().await = Some(message.clone());
                    let _ = task_tx
                        .send(TaskMsg::TtsSpeakResult(Err(
                            crate::ui::task_msg::TtsSpeakErr { message, is_auth },
                        )))
                        .await;
                }
            }
        });
    }
```

Replace the `let text = drill.english.clone(); ... tokio::spawn(...)` block in `speak_current_drill` with:

```rust
        let text = drill.english.clone();
        self.speak_via_tts(text);
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all pre-existing tests still pass — pure refactor.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "refactor(app): extract speak_via_tts from speak_current_drill"
```

---

## Task 8: Wire bundle resolution into `speak_current_drill` (TDD with integration tests)

**Files:**
- Create: `tests/bundled_audio.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Write integration tests (red)**

Create `tests/bundled_audio.rs`:

```rust
//! Integration tests for bundled course audio.
//!
//! Strategy: same `MockSpeaker` pattern as `tts_app_wiring.rs` —
//! count `speak()` calls. When a bundled mp3 is available for the
//! current drill, the mock speaker's `speak()` must NOT be called.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use inkworm::app::App;
use inkworm::audio::player::BundlePlayer;
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::{save_course, Course};
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

fn seed_course(paths: &DataPaths) -> Course {
    let base = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    let course: Course = serde_json::from_str(&base).unwrap();
    save_course(&paths.courses_dir, &course).unwrap();
    course
}

fn make_app(paths: DataPaths, speaker: Arc<dyn Speaker>, course: Course) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    let mut progress = Progress::empty();
    progress.active_course_id = Some(course.id.clone());
    let mut config = Config::default();
    config.tts.r#override = inkworm::config::TtsOverride::On;
    config.tts.iflytek.app_id = "test-app".into();
    config.tts.iflytek.api_key = "test-key".into();
    config.tts.iflytek.api_secret = "test-secret".into();
    let bundle_player = Arc::new(BundlePlayer::new(None));
    App::new(
        Some(course),
        progress,
        paths,
        Arc::new(SystemClock),
        config,
        inkworm::storage::mistakes::MistakeBook::empty(),
        None,
        task_tx,
        speaker,
        bundle_player,
    )
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(40)).await;
}

/// Write `<courses_dir>/<yyyy-mm>/<id_tail>/s{order:02}-d{stage}.mp3`
/// using the silence fixture. Caller specifies the course id.
fn place_bundle_file(courses_dir: &std::path::Path, course_id: &str, order: u32, stage: u32) {
    assert!(course_id.len() >= 11);
    let yyyy_mm = &course_id[0..7];
    let tail = &course_id[8..];
    let dir = courses_dir.join(yyyy_mm).join(tail);
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = std::fs::read("fixtures/audio/silence.mp3").unwrap();
    std::fs::write(dir.join(format!("s{:02}-d{}.mp3", order, stage)), &bytes).unwrap();
}

#[tokio::test]
async fn bundled_hit_skips_speaker() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    // Bundle the very first drill the app will speak on startup
    let order0 = course.sentences[0].order;
    let stage0 = course.sentences[0].drills[0].stage;
    place_bundle_file(&paths.courses_dir, &course.id, order0, stage0);

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let app = make_app(paths, mock.clone(), course);
    app.speak_current_drill();
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    assert!(
        speaks.is_empty(),
        "speaker.speak must not be called when bundle is present, got {speaks:?}"
    );
}

#[tokio::test]
async fn bundled_miss_falls_through_to_speaker() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    // No bundle dir at all.

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let app = make_app(paths, mock.clone(), course.clone());
    app.speak_current_drill();
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    let expected = course.sentences[0].drills[0].english.clone();
    assert_eq!(
        speaks,
        vec![expected],
        "expected one fall-through speak call"
    );
}

#[tokio::test]
async fn bundled_partial_miss_falls_through_for_missing_drill() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    // Place a bundle for s01-d1 but NOT for s01-d2.
    let order0 = course.sentences[0].order;
    let stage0 = course.sentences[0].drills[0].stage;
    place_bundle_file(&paths.courses_dir, &course.id, order0, stage0);

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let mut app = make_app(paths, mock.clone(), course.clone());
    // Startup speak: bundle hit for s01-d1.
    app.speak_current_drill();
    settle().await;
    assert!(
        spoken.lock().unwrap().is_empty(),
        "startup should hit bundle"
    );

    // Skip to s01-d2 via Tab.
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    let tab = Event::Key(KeyEvent {
        code: KeyCode::Tab,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    app.on_input(tab);
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    let expected = course.sentences[0].drills[1].english.clone();
    assert_eq!(
        speaks,
        vec![expected],
        "fall-through expected for missing drill"
    );
}

#[tokio::test]
async fn corrupt_bundle_does_not_call_speaker() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    // Place a zero-byte mp3 — bundle "exists" so we commit to that path,
    // then decode fails and we accept silence (per spec §7).
    let order0 = course.sentences[0].order;
    let stage0 = course.sentences[0].drills[0].stage;
    let yyyy_mm = &course.id[0..7];
    let tail = &course.id[8..];
    let dir = paths.courses_dir.join(yyyy_mm).join(tail);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("s{:02}-d{}.mp3", order0, stage0)), b"").unwrap();

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let app = make_app(paths, mock.clone(), course);
    app.speak_current_drill();
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    assert!(
        speaks.is_empty(),
        "corrupt bundle must not fall through (spec §7), got {speaks:?}"
    );
}
```

- [ ] **Step 2: Run tests — they should fail**

Run: `cargo test --test bundled_audio`
Expected: all 4 fail. Most likely failure mode is `bundled_hit_skips_speaker` and `bundled_partial_miss_falls_through_for_missing_drill` and `corrupt_bundle_does_not_call_speaker` because the bundle branch isn't wired yet — speaker gets called for everything. `bundled_miss_falls_through_to_speaker` may already pass.

- [ ] **Step 3: Implement the bundle branch in `speak_current_drill`**

In `src/app.rs`, edit `speak_current_drill`. Two insertions:

**(a)** At the top of the method, immediately after `self.speaker.cancel();`, add:
```rust
        self.bundle_player.cancel();
```

**(b)** After the `if !should { return; }` line, before `let text = drill.english.clone();`, insert:

```rust
        // Resolve bundle target before borrowing `drill` further. Two
        // separate `&self.study` borrows are issued sequentially so the
        // borrow checker is happy.
        let active_id = self.study.progress().active_course_id.clone();
        let sentence_order = self.study.current_sentence().map(|s| s.order);
        let bundle_target: Option<(String, u32, u32)> = match (active_id, sentence_order) {
            (Some(cid), Some(order)) => Some((cid, order, drill.stage)),
            _ => None,
        };

        if let Some((cid, order, stage)) = bundle_target {
            if let Ok(path) = crate::audio::bundle::bundle_path(
                &self.data_paths.courses_dir,
                &cid,
                order,
                stage,
            ) {
                if path.exists() {
                    let player = Arc::clone(&self.bundle_player);
                    tokio::spawn(async move {
                        if let Err(e) = player.play(&path).await {
                            tracing::warn!("bundle playback failed: {e}");
                        }
                    });
                    return;
                }
            }
        }
```

- [ ] **Step 4: Run integration tests**

Run: `cargo test --test bundled_audio`
Expected: all 4 pass.

If `bundled_hit_skips_speaker` or `corrupt_bundle_does_not_call_speaker` still fail, double-check the bundle branch returns BEFORE `self.speak_via_tts(text)`.

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all old + 4 new tests pass.

- [ ] **Step 6: Commit**

```bash
git add tests/bundled_audio.rs src/app.rs
git commit -m "feat(audio): play bundled mp3 when present, fall through to TTS"
```

---

## Task 9: Final pre-push hygiene

**Files:** none new

- [ ] **Step 1: Format check**

Run: `cargo fmt --all`
Expected: no diff. If diff, the previous commits should have been formatted; amend the most recent commit only if it's the source of the diff:
```bash
cargo fmt --all
git add -u
git commit --amend --no-edit   # only if changes are formatting-only and were yours
```

(Per repo CLAUDE.md: prefer new commits; only amend formatting if it's the same logical change.)

- [ ] **Step 2: Clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean. Fix any warnings inline; if fixes are non-trivial, commit them as `style: clippy fixes`.

- [ ] **Step 3: Full test suite**

Run: `cargo test --all-targets`
Expected: all green.

- [ ] **Step 4: Final commit (if any)**

Whatever the previous steps produced. If everything was already clean, no commit needed.

---

## Out of Scope (per spec §10)

- Bundle generation tooling (separate concern, external)
- Per-course voice config metadata
- Bundle integrity / version metadata
- "Force bundle / force TTS" toggle
- Audio scrubbing / replay UX

These are tracked in the spec; do not implement here.
