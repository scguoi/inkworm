# iCloud-aware Bundle Prewarm Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make bundled-course audio reliably playable when course mp3s arrive as iCloud `dataless` placeholders: detect non-resident files, concurrently prewarm all placeholders for the active course on startup / course switch, and surface progress in the info banner.

**Architecture:** Three independent units — (a) a pure `is_locally_resident(path) -> bool` predicate based on `MetadataExt::blocks()`; (b) a refactored `spawn_prewarm_course` that takes a generation counter + progress channel and uses `futures::stream::for_each_concurrent(8)` with a `take_while` cancellation gate; (c) `App` orchestration: bump generation, set banner, drop stale messages. The bundle gate uses `is_file() && is_locally_resident()` so rodio never reaches a `File::open` that would block on iCloud.

**Tech Stack:** Rust, tokio (rt + sync), futures::stream, ratatui (banner only — no new widgets), `std::os::unix::fs::MetadataExt` (Unix-only `blocks()` accessor).

**Related spec:** `docs/superpowers/specs/2026-05-11-icloud-aware-prewarm-design.md`.

---

## File Map

- **Modify** `src/audio/bundle.rs`
  - Add `is_locally_resident` (pure predicate, Unix path + non-Unix stub).
  - Tighten `bundle_exists` to `is_file() && is_locally_resident()`.
  - Rewrite `spawn_prewarm_course` signature + body (concurrency, cancellation, progress).
  - Update existing unit tests whose fixtures wrote zero-byte placeholder mp3s.
- **Modify** `src/ui/task_msg.rs`
  - Add `TaskMsg::PrewarmProgress { generation, done, total }`.
  - Add `TaskMsg::PrewarmDone { generation, ok, failed }`.
- **Modify** `src/app.rs`
  - New fields on `App`: `prewarm_generation: Arc<AtomicU64>`, `prewarm_state: Option<PrewarmState>`.
  - New struct `PrewarmState { generation, done, total }` (private, defined near App).
  - Rewrite `spawn_bundle_prewarm` to compute placeholder count, bump generation, set banner, and call new `spawn_prewarm_course` signature.
  - Extend `on_task_msg` match arm for new variants.

No other files (`src/main.rs`, `src/audio/player.rs`, `src/audio/mod.rs`, `Cargo.toml`) need changes. `futures = "0.3"` and `tokio = { features = ["sync", "rt", "macros"] }` are already deps.

---

## Task 1: Add `is_locally_resident` predicate

**Files:**
- Modify: `src/audio/bundle.rs` (add function + tests at the bottom of the existing `tests` module)

- [ ] **Step 1.1: Write the failing tests**

Append these tests to the existing `#[cfg(test)] mod tests { ... }` block at the bottom of `src/audio/bundle.rs`. Find the closing `}` of that module and insert these before it:

```rust
    #[test]
    fn is_locally_resident_false_for_zero_byte_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("placeholder.mp3");
        std::fs::write(&p, b"").unwrap();
        assert!(!is_locally_resident(&p));
    }

    #[test]
    fn is_locally_resident_true_for_nonempty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("real.mp3");
        std::fs::write(&p, b"some bytes").unwrap();
        assert!(is_locally_resident(&p));
    }

    #[test]
    fn is_locally_resident_false_for_missing_path() {
        assert!(!is_locally_resident(std::path::Path::new(
            "/definitely/does/not/exist.mp3",
        )));
    }
```

- [ ] **Step 1.2: Run tests to verify they fail**

Run: `cargo test --lib audio::bundle::tests::is_locally_resident -- --nocapture`
Expected: 3 compile failures with "cannot find function `is_locally_resident` in this scope".

- [ ] **Step 1.3: Write minimal implementation**

In `src/audio/bundle.rs`, add this function after `bundle_exists` (around line 38). Keep the imports at the top of the file unchanged — add a new `use` line just for the Unix metadata trait, inside a `cfg(unix)` block at module scope:

At the top of `src/audio/bundle.rs`, after the existing imports (after `use crate::storage::course::{Course, StorageError};`), add:

```rust
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
```

Then, immediately after the existing `bundle_exists` function, add:

```rust
/// On Unix, returns `true` iff `path.metadata().blocks() > 0`. A
/// regular file that exists logically but has zero physical blocks is
/// an iCloud `dataless` placeholder on macOS — opening it forces a
/// synchronous network fetch. On non-Unix targets there is no such
/// notion, so this function returns `true` for any path whose metadata
/// reads successfully.
///
/// Returns `false` on metadata errors (missing path, permission, etc.).
pub fn is_locally_resident(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        meta.blocks() > 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}
```

- [ ] **Step 1.4: Run tests to verify they pass**

Run: `cargo test --lib audio::bundle::tests::is_locally_resident -- --nocapture`
Expected: 3 passes.

- [ ] **Step 1.5: Commit**

```bash
git add src/audio/bundle.rs
git commit -m "feat(audio): add is_locally_resident predicate for iCloud placeholders"
```

---

## Task 2: Tighten `bundle_exists` and fix affected tests

**Files:**
- Modify: `src/audio/bundle.rs` (rewrite `bundle_exists` body, update two existing tests)

Two existing tests in `src/audio/bundle.rs` create zero-byte mp3 fixtures (`std::fs::write(path, b"").unwrap()`). With the tightened predicate, those will be treated as placeholders and `bundle_exists` will now return `false`. Update both to write non-empty bodies so the tests exercise the joint predicate `is_file() && is_locally_resident()`.

- [ ] **Step 2.1: Write the failing test for the new joint behavior**

Append this test to the existing `tests` module in `src/audio/bundle.rs`:

```rust
    #[test]
    fn bundle_exists_false_for_zero_byte_placeholder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("2026-05").join("06-foo");
        std::fs::create_dir_all(&dir).unwrap();
        // Zero-byte file simulates an iCloud dataless placeholder
        std::fs::write(dir.join("s01-d1.mp3"), b"").unwrap();
        assert!(!bundle_exists(tmp.path(), "2026-05-06-foo", 1, 1));
    }
```

- [ ] **Step 2.2: Run the new test to verify it fails**

Run: `cargo test --lib audio::bundle::tests::bundle_exists_false_for_zero_byte_placeholder -- --nocapture`
Expected: FAIL — current `bundle_exists` returns `true` for zero-byte files because `is_file()` is true.

- [ ] **Step 2.3: Update `bundle_exists` to use the joint predicate**

Replace the body of `bundle_exists` in `src/audio/bundle.rs` (currently lines 33–38) with:

```rust
/// Convenience: returns `true` iff `bundle_path` resolves AND the file
/// exists AND the file is locally resident (not an iCloud placeholder).
/// Any error (invalid id, IO error, placeholder) maps to `false`.
pub fn bundle_exists(courses_dir: &Path, course_id: &str, order: u32, stage: u32) -> bool {
    match bundle_path(courses_dir, course_id, order, stage) {
        Ok(p) => p.is_file() && is_locally_resident(&p),
        Err(_) => false,
    }
}
```

- [ ] **Step 2.4: Update the two existing tests that wrote zero-byte fixtures**

In the same `tests` module of `src/audio/bundle.rs`, find these two tests and change every `b""` to `b"x"`:

Test `bundle_exists_true_when_file_present`: change the `std::fs::write(dir.join("s01-d1.mp3"), b"").unwrap();` line to `std::fs::write(dir.join("s01-d1.mp3"), b"x").unwrap();`.

Test `bundle_exists_false_for_other_stage`: change the `std::fs::write(dir.join("s01-d1.mp3"), b"").unwrap();` line to `std::fs::write(dir.join("s01-d1.mp3"), b"x").unwrap();`.

(Both tests still assert the same outcomes; the body change only makes the underlying file locally resident so `is_locally_resident` doesn't trip them.)

- [ ] **Step 2.5: Run all `bundle_exists` tests to verify they pass**

Run: `cargo test --lib audio::bundle::tests::bundle_exists -- --nocapture`
Expected: 5 passes (the 4 pre-existing tests plus the new `_for_zero_byte_placeholder`).

- [ ] **Step 2.6: Commit**

```bash
git add src/audio/bundle.rs
git commit -m "fix(audio): bundle_exists rejects iCloud placeholders"
```

---

## Task 3: Add new TaskMsg variants

**Files:**
- Modify: `src/ui/task_msg.rs`

- [ ] **Step 3.1: Write a compile-only test asserting the variants exist with the spec'd field shape**

Append this test to a new `#[cfg(test)] mod tests { ... }` block at the bottom of `src/ui/task_msg.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prewarm_variants_construct_as_specified() {
        let progress = TaskMsg::PrewarmProgress {
            generation: 1,
            done: 5,
            total: 10,
        };
        let done = TaskMsg::PrewarmDone {
            generation: 1,
            ok: 9,
            failed: 1,
        };
        // Smoke: Debug derive still works after adding variants.
        let _ = format!("{:?}", progress);
        let _ = format!("{:?}", done);
    }
}
```

- [ ] **Step 3.2: Run to verify failure**

Run: `cargo test --lib ui::task_msg::tests::prewarm_variants_construct_as_specified -- --nocapture`
Expected: FAIL — `no variant or associated item named PrewarmProgress / PrewarmDone found for enum TaskMsg`.

- [ ] **Step 3.3: Add the variants to `TaskMsg`**

In `src/ui/task_msg.rs`, modify the `TaskMsg` enum (currently lines 6–12). Add two variants so the enum reads:

```rust
/// Messages sent from background tasks to the main event loop.
#[derive(Debug)]
pub enum TaskMsg {
    Generate(GenerateProgress),
    Wizard(WizardTaskMsg),
    DeviceDetected(OutputKind),
    TtsSpeakResult(Result<(), TtsSpeakErr>),
    /// Prewarm of bundled-course audio has made progress.
    /// `done` is the count of files processed (succeeded + failed) so far;
    /// `total` is the placeholder count when the run started.
    PrewarmProgress {
        generation: u64,
        done: u32,
        total: u32,
    },
    /// Prewarm finished (or was cancelled by a newer generation).
    PrewarmDone {
        generation: u64,
        ok: u32,
        failed: u32,
    },
}
```

- [ ] **Step 3.4: Run to verify pass**

Run: `cargo test --lib ui::task_msg::tests::prewarm_variants_construct_as_specified -- --nocapture`
Expected: PASS.

- [ ] **Step 3.5: Confirm match exhaustiveness elsewhere does not break the build**

Run: `cargo build`
Expected: a compile error on `src/app.rs` for the non-exhaustive `match msg { ... }` inside `on_task_msg` (since we added two variants but haven't handled them yet). **This is expected.** Do NOT fix it here — Task 6 owns those handlers. Continue without fixing.

- [ ] **Step 3.6: Commit**

```bash
git add src/ui/task_msg.rs
git commit -m "feat(task_msg): add PrewarmProgress and PrewarmDone variants"
```

(The repo will be in a non-compiling state until Task 6; Task 4 fixes `spawn_prewarm_course` and Task 5 adds the App-side spawn, and Task 6 closes the loop. Tasks 4-6 are a logical bundle. Commits between them are fine because we keep moving in one direction; bisect for unrelated bugs in this window is unlikely.)

---

## Task 4: Refactor `spawn_prewarm_course` to be concurrent + cancellable

**Files:**
- Modify: `src/audio/bundle.rs`

The new signature takes a generation counter, the current-generation atomic, and a progress channel. The body uses `futures::stream::iter(paths).take_while(...).for_each_concurrent(Some(8), ...)`. Per-file work is `tokio::task::spawn_blocking(move || materialize(&p))`. Atomic counters track `done` and `failed`. After the stream resolves (or the take_while gate trips), a final `PrewarmDone` is sent.

- [ ] **Step 4.1: Add imports needed by the new implementation**

At the top of `src/audio/bundle.rs`, add these `use` lines after the existing `use crate::storage::course::{Course, StorageError};`:

```rust
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use tokio::sync::mpsc;

use crate::ui::task_msg::TaskMsg;
```

- [ ] **Step 4.2: Write the new test for cancellation via generation**

Append this test to the `tests` module in `src/audio/bundle.rs`:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_prewarm_course_stops_when_generation_advances() {
        // Build a course referencing 16 mp3 paths under a tempdir;
        // none of them exist on disk, so each materialize errors quickly
        // and the loop should proceed fast. Bumping the generation to a
        // different value before spawning means the take_while gate trips
        // on the very first poll and no progress messages are sent.
        let tmp = tempfile::tempdir().unwrap();
        let course = course_with_drills(
            "2026-05-06-foo",
            &[
                &[1, 2, 3, 4],
                &[1, 2, 3, 4],
                &[1, 2, 3, 4],
                &[1, 2, 3, 4],
            ],
        );

        let gen = Arc::new(AtomicU64::new(2)); // current is 2
        let (tx, mut rx) = mpsc::channel::<TaskMsg>(64);

        // Spawn with stale generation 1 — must short-circuit.
        spawn_prewarm_course(tmp.path().to_path_buf(), &course, 1, gen.clone(), tx);

        // Drain channel with a tight timeout. We expect exactly one
        // PrewarmDone with ok=0 failed=0 (nothing was attempted).
        let mut got_done = false;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some(TaskMsg::PrewarmDone { generation: 1, ok: 0, failed: 0 })) => {
                    got_done = true;
                    break;
                }
                Ok(Some(other)) => panic!("unexpected message: {other:?}"),
                Ok(None) => break,
                Err(_) => break, // timeout
            }
        }
        assert!(got_done, "expected PrewarmDone with ok=0 failed=0");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_prewarm_course_reports_progress_and_done() {
        // We deliberately do NOT create the mp3 files. Missing files mean
        // `is_locally_resident` returns false (metadata error) so they pass
        // the "skip resident" filter and reach `materialize`, which then
        // fails with NotFound. This exercises the full pipeline — take_while
        // gate → for_each_concurrent → spawn_blocking → atomic increment →
        // Progress send → final Done — with each file counted as `failed`.
        //
        // Creating real files (even 1 byte) would make them locally resident
        // on APFS/ext4 (allocated blocks > 0), get filtered out before the
        // stream, and result in total=0 — bypassing the progress code path.
        let tmp = tempfile::tempdir().unwrap();
        let course = course_with_drills("2026-05-06-foo", &[&[1, 2], &[1]]);
        let gen = Arc::new(AtomicU64::new(7));
        let (tx, mut rx) = mpsc::channel::<TaskMsg>(64);

        spawn_prewarm_course(tmp.path().to_path_buf(), &course, 7, gen.clone(), tx);

        let mut progress_count = 0u32;
        let mut max_progress_done = 0u32;
        let mut done_msg: Option<TaskMsg> = None;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some(TaskMsg::PrewarmProgress {
                    generation: 7,
                    done,
                    total: 3,
                })) => {
                    progress_count += 1;
                    if done > max_progress_done {
                        max_progress_done = done;
                    }
                }
                Ok(Some(msg @ TaskMsg::PrewarmDone { generation: 7, .. })) => {
                    done_msg = Some(msg);
                    break;
                }
                Ok(Some(other)) => panic!("unexpected msg: {other:?}"),
                Ok(None) | Err(_) => break,
            }
        }

        // Exactly one Progress per file. The `done` value carried by each
        // message depends on scheduler interleaving (Relaxed atomic reads
        // can observe concurrent increments out of order), so we don't
        // assert a specific value per-message — only the count and the
        // final authoritative tally in PrewarmDone.
        assert_eq!(progress_count, 3, "expected 3 progress messages");
        assert!(max_progress_done <= 3, "progress.done must never exceed total");
        match done_msg.expect("expected a PrewarmDone") {
            TaskMsg::PrewarmDone { generation: 7, ok: 0, failed: 3 } => {}
            other => panic!("unexpected done: {other:?}"),
        }
    }
```

Note: the helper `course_with_drills` already exists in the same `tests` module (lines 110–142 of `src/audio/bundle.rs`) and can be reused as-is.

- [ ] **Step 4.3: Run tests to verify failure**

Run: `cargo test --lib audio::bundle::tests::spawn_prewarm_course -- --nocapture`
Expected: compile errors (signature mismatch) and/or test failures (current `spawn_prewarm_course` never sends `TaskMsg`).

- [ ] **Step 4.4: Rewrite `spawn_prewarm_course`**

Replace the existing `spawn_prewarm_course` function in `src/audio/bundle.rs` (currently lines 73–92) with this new implementation. The doc comment is updated to match.

```rust
/// Concurrently materialize every drill mp3 for `course` from iCloud,
/// reporting progress through `progress_tx`. The work runs on a tokio
/// task spawned via `Handle::try_current()`; calling outside a runtime
/// is a silent no-op (the function never panics).
///
/// **Cancellation:** before each new file enters the work pool, the
/// task checks `current_generation.load(Acquire)`. If it no longer
/// equals `generation`, the stream short-circuits and no further files
/// are touched. In-flight blocking jobs (≤ 8) run to completion.
///
/// **Progress:** one `PrewarmProgress` per file (success or failure);
/// one final `PrewarmDone` carrying `ok` and `failed` counters.
///
/// **Skipping resident files:** already-resident files are filtered out
/// up-front, so `total` in progress messages reflects placeholder count,
/// not the full drill count.
pub fn spawn_prewarm_course(
    courses_dir: PathBuf,
    course: &Course,
    generation: u64,
    current_generation: Arc<AtomicU64>,
    progress_tx: mpsc::Sender<TaskMsg>,
) {
    let all_paths = bundle_paths_for_course(&courses_dir, course);
    let paths: Vec<PathBuf> = all_paths
        .into_iter()
        .filter(|p| !is_locally_resident(p))
        .collect();
    let total = paths.len() as u32;

    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };

    handle.spawn(async move {
        let ok = Arc::new(AtomicU32::new(0));
        let failed = Arc::new(AtomicU32::new(0));

        if total > 0 {
            stream::iter(paths)
                .take_while(move |_| {
                    let alive = current_generation.load(Ordering::Acquire) == generation;
                    async move { alive }
                })
                .for_each_concurrent(Some(8), |path| {
                    let ok = ok.clone();
                    let failed = failed.clone();
                    let progress_tx = progress_tx.clone();
                    async move {
                        let p = path.clone();
                        let join = tokio::task::spawn_blocking(move || materialize(&p)).await;
                        match join {
                            Ok(Ok(n)) => {
                                ok.fetch_add(1, Ordering::Relaxed);
                                tracing::debug!(
                                    "bundle prewarm {}: {} bytes",
                                    path.display(),
                                    n
                                );
                            }
                            Ok(Err(e)) => {
                                failed.fetch_add(1, Ordering::Relaxed);
                                tracing::warn!(
                                    "bundle prewarm failed {}: {}",
                                    path.display(),
                                    e
                                );
                            }
                            Err(e) => {
                                failed.fetch_add(1, Ordering::Relaxed);
                                tracing::warn!(
                                    "bundle prewarm join error for {}: {}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                        let done =
                            ok.load(Ordering::Relaxed) + failed.load(Ordering::Relaxed);
                        let _ = progress_tx
                            .send(TaskMsg::PrewarmProgress {
                                generation,
                                done,
                                total,
                            })
                            .await;
                    }
                })
                .await;
        }

        let _ = progress_tx
            .send(TaskMsg::PrewarmDone {
                generation,
                ok: ok.load(Ordering::Relaxed),
                failed: failed.load(Ordering::Relaxed),
            })
            .await;
    });
}
```

- [ ] **Step 4.5: Update the existing `spawn_prewarm_course_outside_runtime_does_not_panic` test for the new signature**

In `src/audio/bundle.rs`, find the test `spawn_prewarm_course_outside_runtime_does_not_panic` (around line 225 of the old file) and replace its body with the new call shape:

```rust
    #[test]
    fn spawn_prewarm_course_outside_runtime_does_not_panic() {
        let course = course_with_drills("2026-05-06-foo", &[&[1]]);
        let gen = Arc::new(AtomicU64::new(1));
        let (tx, _rx) = mpsc::channel::<TaskMsg>(4);
        // Called from a plain #[test] = no tokio runtime in scope.
        spawn_prewarm_course(
            PathBuf::from("/tmp/no-such-courses"),
            &course,
            1,
            gen,
            tx,
        );
    }
```

- [ ] **Step 4.6: Run all audio::bundle tests to verify everything passes**

Run: `cargo test --lib audio::bundle -- --nocapture`
Expected: all tests pass — `is_locally_resident_*` (3), `bundle_path_*` (3), `bundle_exists_*` (5), `bundle_paths_for_course_*` (2), `materialize_*` (2), `spawn_prewarm_course_outside_runtime_does_not_panic` (1), `spawn_prewarm_course_stops_when_generation_advances` (1), `spawn_prewarm_course_reports_progress_and_done` (1).

Total ~18 tests in `audio::bundle::tests`.

- [ ] **Step 4.7: Commit**

```bash
git add src/audio/bundle.rs
git commit -m "feat(audio): prewarm runs concurrently with generation-based cancellation"
```

---

## Task 5: Wire prewarm fields into `App` and rewrite `spawn_bundle_prewarm`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 5.1: Add imports and the `PrewarmState` struct**

In `src/app.rs`, find the import block at the top (the first ~30 lines). Confirm `use std::sync::Arc;` is already present (it is — see line where `Arc<Mutex<...>>` is constructed). Then add to the import block:

```rust
use std::sync::atomic::AtomicU64;
```

Then, immediately before the `pub struct App { ... }` declaration, add this private struct:

```rust
/// Snapshot of an in-progress bundle prewarm run, used to drive the
/// info banner. Only ever touched on the main loop thread; the
/// generation match guards against messages from cancelled runs.
#[derive(Debug, Clone, Copy)]
struct PrewarmState {
    generation: u64,
    done: u32,
    total: u32,
}
```

- [ ] **Step 5.2: Add the two new fields to `App`**

In `src/app.rs`, find the `pub struct App { ... }` definition (around line 38). Add these two fields just before the closing brace of the struct (after `pub shell_header: ...`):

```rust
    prewarm_generation: Arc<AtomicU64>,
    prewarm_state: Option<PrewarmState>,
```

- [ ] **Step 5.3: Initialize the new fields in `App::new`**

In `src/app.rs`, find `impl App { pub fn new(...) -> Self { let mut app = Self { ... } }` (the struct literal starts around line 79). Add these two field initializers inside the struct literal, immediately after `shell_header: crate::ui::shell_chrome::ShellHeader::detect(),`:

```rust
            prewarm_generation: Arc::new(AtomicU64::new(0)),
            prewarm_state: None,
```

- [ ] **Step 5.4: Rewrite `spawn_bundle_prewarm`**

In `src/app.rs`, find the existing `fn spawn_bundle_prewarm(&self)` method (around line 131). Replace it with this new implementation. Note: the receiver becomes `&mut self` because we now mutate `prewarm_state` and `info_banner`.

```rust
    /// Kick off background materialization of the active course's
    /// bundle mp3s, with concurrency, cancellation, and UI progress.
    /// Cheap no-op when no active course or every file is already
    /// locally resident.
    fn spawn_bundle_prewarm(&mut self) {
        let Some(course) = self.study.current_course() else {
            return;
        };
        let courses_dir = self.data_paths.courses_dir.clone();
        let course_owned = course.clone();

        // Count placeholders before bumping generation, so a 0-placeholder
        // run is a true no-op (no banner flicker, no spawned task).
        let total = crate::audio::bundle::bundle_paths_for_course(&courses_dir, &course_owned)
            .iter()
            .filter(|p| !crate::audio::bundle::is_locally_resident(p))
            .count() as u32;
        if total == 0 {
            tracing::debug!("bundle prewarm: nothing to do");
            return;
        }

        let generation =
            self.prewarm_generation.fetch_add(1, std::sync::atomic::Ordering::Release) + 1;
        self.prewarm_state = Some(PrewarmState {
            generation,
            done: 0,
            total,
        });
        self.info_banner = Some(format!("Prewarming audio (0/{})…", total));
        tracing::info!(
            "bundle prewarm start: generation={} total={}",
            generation,
            total
        );

        crate::audio::bundle::spawn_prewarm_course(
            courses_dir,
            &course_owned,
            generation,
            self.prewarm_generation.clone(),
            self.task_tx.clone(),
        );
    }
```

- [ ] **Step 5.5: Tighten the inline bundle gate in `speak_current_drill`**

Spec §3.1 assumes `bundle_exists` is the gate consulted by `App::speak_current_drill`, but the actual gate is an inline `path.exists()` check at `src/app.rs:313`. Without this fix, tightening `bundle_exists` alone does nothing for the playback path. Replace the inline check:

In `src/app.rs`, find the block inside `speak_current_drill` (around lines 309–323):

```rust
        if let Some((cid, order, stage)) = bundle_target {
            if let Ok(path) =
                crate::audio::bundle::bundle_path(&self.data_paths.courses_dir, &cid, order, stage)
            {
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

Change exactly one line — `if path.exists() {` becomes:

```rust
                if path.is_file() && crate::audio::bundle::is_locally_resident(&path) {
```

Rationale for inlining instead of calling `bundle_exists`: we already have `path` materialized; calling `bundle_exists` would re-run `bundle_path` and re-parse the course id. The composite predicate here is identical to what `bundle_exists` returns.

- [ ] **Step 5.6: Fix the three existing callers that now need `&mut self`**

There are three call sites for `spawn_bundle_prewarm`. Two are inside methods that already have `&mut self`, but one is `App::new` which uses `let mut app = Self { ... }; ... app.spawn_bundle_prewarm();` — that's fine because `app` is mutable.

Verify by running:

Run: `grep -n "spawn_bundle_prewarm" src/app.rs`
Expected three lines:
1. The definition (now `fn spawn_bundle_prewarm(&mut self)`).
2. `app.spawn_bundle_prewarm();` inside `App::new` — works (app is mut).
3. `self.spawn_bundle_prewarm();` inside `enter_mistakes_mode_at_current_drill` — that method is `&mut self`, works.
4. `self.spawn_bundle_prewarm();` inside `switch_to_course` — that method is `&mut self`, works.

If you see compile errors in this step about mutable borrow conflicts, the most likely cause is `self.study.current_course()` returning a borrow that conflicts. The fix is already baked into the implementation in 5.4: we clone the course out via `course.clone()` and drop the borrow before mutating `self.info_banner`.

- [ ] **Step 5.7: Build the crate to confirm everything compiles except the still-non-exhaustive match in `on_task_msg`**

Run: `cargo build 2>&1 | head -40`
Expected: the only remaining error should be the non-exhaustive `match msg { ... }` inside `on_task_msg` for the two new variants. No other errors. (Task 6 closes this.)

- [ ] **Step 5.8: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): prewarm orchestration plus iCloud-resident bundle gate"
```

---

## Task 6: Handle `PrewarmProgress` and `PrewarmDone` in `on_task_msg`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 6.1: Write the integration test**

Append this test to the existing `#[cfg(test)] mod tests` block at the bottom of `src/app.rs`. If that module doesn't exist, scroll to the end of the file and add a new one. We need a way to drive `on_task_msg` against a minimally-constructed `App`. The test sets `prewarm_state` directly to bypass the spawn path.

If `src/app.rs` already has a `#[cfg(test)] mod tests` block, append to it. Otherwise, add a new one at the very bottom of the file:

```rust
#[cfg(test)]
mod prewarm_msg_tests {
    use super::*;
    use crate::ui::task_msg::TaskMsg;

    fn app_with_prewarm(state: Option<PrewarmState>, gen: u64) -> App {
        // Build a minimal App via App::test_fixture if one exists, else
        // skip — the on_task_msg logic for these variants depends only
        // on prewarm_state, prewarm_generation, and info_banner, so we
        // construct just those fields ad-hoc using a helper.
        let mut app = App::test_fixture_minimal();
        app.prewarm_state = state;
        app.prewarm_generation
            .store(gen, std::sync::atomic::Ordering::Release);
        app
    }

    #[test]
    fn progress_with_current_generation_updates_banner() {
        let state = PrewarmState {
            generation: 3,
            done: 0,
            total: 5,
        };
        let mut app = app_with_prewarm(Some(state), 3);
        app.on_task_msg(TaskMsg::PrewarmProgress {
            generation: 3,
            done: 2,
            total: 5,
        });
        assert_eq!(
            app.info_banner.as_deref(),
            Some("Prewarming audio (2/5)…")
        );
        assert_eq!(app.prewarm_state.unwrap().done, 2);
    }

    #[test]
    fn progress_with_stale_generation_is_dropped() {
        let state = PrewarmState {
            generation: 5,
            done: 1,
            total: 5,
        };
        let mut app = app_with_prewarm(Some(state), 5);
        app.info_banner = Some("kept".into());
        app.on_task_msg(TaskMsg::PrewarmProgress {
            generation: 4,
            done: 3,
            total: 5,
        });
        assert_eq!(app.info_banner.as_deref(), Some("kept"));
        assert_eq!(app.prewarm_state.unwrap().done, 1);
    }

    #[test]
    fn done_with_no_failures_clears_banner() {
        let state = PrewarmState {
            generation: 9,
            done: 5,
            total: 5,
        };
        let mut app = app_with_prewarm(Some(state), 9);
        app.info_banner = Some("Prewarming audio (5/5)…".into());
        app.on_task_msg(TaskMsg::PrewarmDone {
            generation: 9,
            ok: 5,
            failed: 0,
        });
        assert!(app.prewarm_state.is_none());
        assert!(app.info_banner.is_none());
    }

    #[test]
    fn done_with_failures_sets_failure_banner() {
        let state = PrewarmState {
            generation: 1,
            done: 5,
            total: 5,
        };
        let mut app = app_with_prewarm(Some(state), 1);
        app.on_task_msg(TaskMsg::PrewarmDone {
            generation: 1,
            ok: 3,
            failed: 2,
        });
        assert!(app.prewarm_state.is_none());
        assert_eq!(
            app.info_banner.as_deref(),
            Some("Audio ready (3/5), 2 files unavailable")
        );
    }

    #[test]
    fn done_with_stale_generation_is_dropped() {
        let state = PrewarmState {
            generation: 2,
            done: 5,
            total: 5,
        };
        let mut app = app_with_prewarm(Some(state), 2);
        app.info_banner = Some("kept".into());
        app.on_task_msg(TaskMsg::PrewarmDone {
            generation: 1,
            ok: 5,
            failed: 0,
        });
        assert!(app.prewarm_state.is_some());
        assert_eq!(app.info_banner.as_deref(), Some("kept"));
    }
}
```

The helper `App::test_fixture_minimal()` doesn't exist yet — Step 6.2 adds it.

- [ ] **Step 6.2: Add the test fixture constructor on `App`**

In `src/app.rs`, add this `#[cfg(test)]` impl block immediately after the existing `impl App { ... }` block (or anywhere at the top level of the file, as long as it's not inside another module):

```rust
#[cfg(test)]
impl App {
    /// Minimal App used by prewarm message-handling tests. Fields not
    /// exercised by those tests are filled with defaults / stubs that
    /// will panic if accidentally used. Do not extend without thought —
    /// this is not a general-purpose test fixture.
    fn test_fixture_minimal() -> Self {
        use crate::clock::SystemClock;
        use crate::config::Config;
        use crate::storage::mistakes::MistakeBook;
        use crate::storage::paths::DataPaths;
        use crate::storage::progress::Progress;
        use crate::tts::speaker::Speaker;
        use std::sync::Arc;

        struct NoopSpeaker;
        #[async_trait::async_trait]
        impl Speaker for NoopSpeaker {
            async fn speak(
                &self,
                _text: &str,
            ) -> Result<(), crate::tts::speaker::TtsError> {
                Ok(())
            }
        }

        let (task_tx, _rx) = tokio::sync::mpsc::channel(8);
        let tmp = std::env::temp_dir().join("inkworm-test-fixture");
        let data_paths = DataPaths::for_tests(tmp);
        let progress = Progress::default();
        let bundle_player = Arc::new(crate::audio::player::BundlePlayer::new(None));

        Self::new(
            None,
            progress,
            data_paths,
            Arc::new(SystemClock),
            Config::default(),
            MistakeBook::default(),
            None,
            task_tx,
            Arc::new(NoopSpeaker) as Arc<dyn Speaker>,
            bundle_player,
        )
    }
}
```

Note: `Config::default()`, `MistakeBook::default()`, `Progress::default()`, `DataPaths::for_tests` all already exist (verified earlier in spec writing). If any one is missing a `Default` impl, derive `#[derive(Default)]` on the simplest one in a follow-up — but `Progress` and `MistakeBook` already have `Default`, `Config` has `Default`, and `DataPaths::for_tests` is the existing test constructor.

- [ ] **Step 6.3: Run the new tests to verify they fail**

Run: `cargo test --lib app::prewarm_msg_tests -- --nocapture`
Expected: compile failure because `PrewarmProgress`/`PrewarmDone` are unhandled in `on_task_msg`, plus assertion failures once compilation succeeds. The compilation failure is the first thing to fix.

- [ ] **Step 6.4: Extend `on_task_msg` with two new match arms**

In `src/app.rs`, find the `match msg { ... }` inside `pub fn on_task_msg(&mut self, msg: TaskMsg)` (around line 480). Add these two arms immediately before the closing brace of the match:

```rust
            TaskMsg::PrewarmProgress {
                generation,
                done,
                total,
            } => {
                if let Some(state) = self.prewarm_state.as_mut() {
                    if state.generation == generation {
                        state.done = done;
                        state.total = total;
                        self.info_banner =
                            Some(format!("Prewarming audio ({}/{})…", done, total));
                    }
                }
            }
            TaskMsg::PrewarmDone {
                generation,
                ok,
                failed,
            } => {
                let matches = self
                    .prewarm_state
                    .as_ref()
                    .map(|s| s.generation == generation)
                    .unwrap_or(false);
                if matches {
                    self.prewarm_state = None;
                    if failed == 0 {
                        self.info_banner = None;
                        tracing::info!(
                            "bundle prewarm done: generation={} ok={} failed=0",
                            generation,
                            ok
                        );
                    } else {
                        let total = ok + failed;
                        self.info_banner = Some(format!(
                            "Audio ready ({}/{}), {} files unavailable",
                            ok, total, failed
                        ));
                        tracing::warn!(
                            "bundle prewarm done with failures: generation={} ok={} failed={}",
                            generation,
                            ok,
                            failed
                        );
                    }
                }
            }
```

- [ ] **Step 6.5: Run the new tests to verify they pass**

Run: `cargo test --lib app::prewarm_msg_tests -- --nocapture`
Expected: all 5 tests pass.

- [ ] **Step 6.6: Run the full test suite to make sure nothing else broke**

Run: `cargo test --lib -- --nocapture 2>&1 | tail -30`
Expected: zero failures. If anything fails, fix it before continuing.

- [ ] **Step 6.7: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): handle PrewarmProgress and PrewarmDone task messages"
```

---

## Task 7: Build, lint, manual smoke

**Files:**
- No code changes; this task verifies the full build is clean and the actual user-reported scenario plays audio.

- [ ] **Step 7.1: Run `cargo fmt --check` on changed files**

Per `MEMORY.md` (feedback_cargo_fmt_check_bug.md): `cargo fmt --check` ignores file args; use `rustfmt --check` per-file.

Run:
```bash
rustfmt --check src/audio/bundle.rs src/ui/task_msg.rs src/app.rs
```
Expected: no output (clean). If any file is unformatted, run `rustfmt src/<file>.rs` and commit the format fix as a separate commit: `style: rustfmt`.

- [ ] **Step 7.2: Run `cargo clippy` with `-D warnings`**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -30`
Expected: no warnings. Address anything clippy flags before continuing; do not allow new lints.

- [ ] **Step 7.3: Run the full test suite once more**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass. Note the suite includes integration tests (under `tests/`) — make sure none of them broke from the bundle/app changes.

- [ ] **Step 7.4: Install locally for smoke testing**

Per project CLAUDE.md (post-release step, applied here for the dev build):

```bash
cargo install --path . --force
```

Expected: install completes; `~/.cargo/bin/inkworm` updated.

- [ ] **Step 7.5: Smoke verify — prewarm actually downloads the user's stuck course**

Pre-state: `~/.config/inkworm/courses/2026-05/10-talk-about-yourself-with-confidence/` has 37 placeholder mp3s (blocks=0). Verify:

```bash
stat -f "%-12N blocks=%b" ~/.config/inkworm/courses/2026-05/10-talk-about-yourself-with-confidence/*.mp3 | grep "blocks=0" | wc -l
```

Expected: should be 37 (the prewarm hasn't run yet under the new binary). (If it's already 0, the user re-warmed manually — skip the smoke test pre-condition reset, but still verify the post-condition.)

Start inkworm interactively:

```bash
inkworm
```

Watch the info banner. Expected behavior:
1. Banner shows `Prewarming audio (0/N)…` (N around 37) within ~1 second of startup.
2. Banner counter advances `(X/37)…` as files arrive.
3. Banner clears when prewarm finishes (or shows `Audio ready (K/37), M files unavailable` if some fail).
4. Press a key (any non-quit) — banner disappears via the existing one-keypress dismissal.

Quit inkworm (`:q` or `Ctrl-C`).

- [ ] **Step 7.6: Verify post-condition**

Run:
```bash
stat -f "%-12N blocks=%b" ~/.config/inkworm/courses/2026-05/10-talk-about-yourself-with-confidence/*.mp3 | awk '$2 == "blocks=0"' | wc -l
```

Expected: `0` (or near 0 if a few failed). Meaning every mp3 is now locally resident.

- [ ] **Step 7.7: Smoke verify — audio actually plays**

Start inkworm again with AirPods connected:
```bash
inkworm
```

Switch to the previously-stuck course (`:open` or the course list, depending on UX). The first drill should play the bundled mp3 — you should hear voice within a second. Step through 3–4 drills to make sure subsequent files also play (not just the warmed first 3).

If audio plays consistently, the fix is verified.

- [ ] **Step 7.8: Commit any straggling format/lint fixes from 7.1 / 7.2 if there were any**

If Steps 7.1 or 7.2 produced commits, fine. Otherwise nothing to do here.

- [ ] **Step 7.9: Final sanity — `git status` clean**

Run: `git status`
Expected: `nothing to commit, working tree clean`.

---

## Self-Review

**Spec coverage:**

- §3.1 `is_locally_resident` → Task 1 ✓
- §3.1 `bundle_exists` tightening → Task 2 ✓
- §3.2 `spawn_prewarm_course` new signature, skip-resident, take_while gate, for_each_concurrent(8), per-file warn, final Done → Task 4 ✓
- §3.3 `App` fields `prewarm_generation`/`prewarm_state` → Task 5 ✓
- §3.3 `spawn_bundle_prewarm` rewrite (count placeholders, bump generation, set banner, no-op when total=0) → Task 5 ✓
- §3.3 three existing call sites unchanged → Task 5 verification step ✓
- §5 TaskMsg additions → Task 3 ✓
- §5 handling in `on_task_msg` (progress banner, done-with/without failures, generation guard) → Task 6 ✓
- §6 cancellation semantics → Task 4 (take_while) + Task 6 (stale-generation drop) ✓
- §7 behavior matrix → Tasks 1+2+4+6 in combination ✓
- §8 testing → Task 1 (3 tests), Task 2 (1 new + 2 updated), Task 4 (3 tests), Task 6 (5 tests) ✓
- §9 migration: no schema changes, just one extra prewarm pass after upgrade → Task 7.5–7.7 smoke ✓
- §10 rejected alternatives → none of these are implemented (correct) ✓

All spec sections have at least one implementing task.

**Placeholder scan:** No TBDs, TODOs, "implement later", or "similar to" references. Every code block is concrete.

**Type consistency:**

- `TaskMsg::PrewarmProgress { generation: u64, done: u32, total: u32 }` — same in Task 3 (definition), Task 4 (sends), Task 6 (matches).
- `TaskMsg::PrewarmDone { generation: u64, ok: u32, failed: u32 }` — same across all three tasks.
- `Arc<AtomicU64>` for `prewarm_generation` — Task 5 (definition + Arc::new), Task 4 (parameter type `Arc<AtomicU64>`), Task 5 spawn site (`self.prewarm_generation.clone()`). ✓
- `Arc<AtomicU32>` for ok/failed counters — Task 4 internal only. ✓
- `bundle_paths_for_course` — used in Task 4 (filter for total) and Task 5 (count for total). Function signature `(courses_dir: &Path, course: &Course) -> Vec<PathBuf>` matches both call sites. ✓
- `is_locally_resident(path: &Path) -> bool` — signature consistent in Task 1 (impl), Task 2 (call), Task 4 (call), Task 5 (call). ✓
- `spawn_bundle_prewarm` becomes `&mut self` in Task 5; all three existing call sites already operate on mutable bindings (`mut app` in `App::new`, `&mut self` in the two methods). Verified in Step 5.5. ✓

No type mismatches.
