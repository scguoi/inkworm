# Learning Statistics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Per-day aggregated learning statistics (active time, submits, accuracy, words) viewable via a full-screen `/stats` palette command and a one-line today strip on the Study screen.

**Architecture:** New `src/stats/` module (pure logic: `StatsTracker`, `aggregate`) and `src/storage/stats.rs` (serde + atomic save). App owns one `StatsTracker`; Study screen keystrokes / submits drive it; tick calls `flush_idle` to close idle sessions. New `Screen::Stats` renders the full view; a mini-strip is added to `App::render_bottom_banners`.

**Tech Stack:** Rust, Ratatui 0.29, chrono, serde_json, tokio (existing), insta (dev — for any string snapshots; UI rendering uses TestBackend buffer assertions).

**Related spec:** `docs/superpowers/specs/2026-05-13-learning-stats-design.md` (Approved 2026-05-13).

**Decisions baked into this plan (from the spec, not negotiable):**

- Active time = keystroke-driven, accumulated only when delta ≤ 30 s (`IDLE_THRESHOLD_MS = 30_000`).
- Words = `drill.english.split_whitespace().count()` per submitted drill.
- Accuracy = `correct_submits / total_submits` — every `submit()` counted, retries included.
- Course mode and Mistakes mode contribute to the same totals.
- Per-day aggregates persisted forever in `~/.config/inkworm/stats.json`.
- ISO 8601 weeks (Mon–Sun). `recent_weeks` excludes the current week (already shown in "This week"); `recent_months` includes the current month.

**Pre-flight check (read before starting):**

- `src/storage/progress.rs` — mirror its structure for `Stats` (schema version, `BTreeMap` keys, `write_atomic`, missing-file → `empty()`).
- `src/storage/paths.rs` — `DataPaths` is the place to add the `stats_file` field.
- `src/clock.rs` — use `Clock` trait everywhere time matters; tests use `FixedClock`.
- `src/ui/study.rs::SubmitOutcome` — current signature drops "non-first" submits; we extend `submit()` to also return a `SubmitTick` on every call.
- `src/app.rs::handle_study_key` (line ~690) — the one place to install the keystroke hook (Study screen only).
- `src/app.rs::render_bottom_banners` (line ~1206) — banners stack from `last_row` up using `row_from_bottom`. The mini-strip lives in this stack on `Screen::Study`.

**Hygiene at the end (Task 10):** `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. The repo's CI runs all three on macos-14; pre-push hook does the same. Conventional Commits, English only. Stage explicit file paths, never `git add .`.

---

### Task 1: `Stats` data model + `DataPaths::stats_file`

**Files:**
- Create: `src/storage/stats.rs`
- Modify: `src/storage/paths.rs` (add `stats_file` field)
- Modify: `src/storage/mod.rs` (declare `pub mod stats;`)

- [ ] **Step 1: Add `stats_file` to `DataPaths`**

Edit `src/storage/paths.rs`. Add `stats_file: PathBuf,` to the struct just below `lock_file` (line 18), and inside `from_root` (line 45) add `stats_file: root.join("stats.json"),` alongside the other join lines. No new `create_dir_all` is needed — the file is created on first save.

After edit, the struct should read:

```rust
pub struct DataPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub progress_file: PathBuf,
    pub mistakes_file: PathBuf,
    pub log_file: PathBuf,
    pub lock_file: PathBuf,
    pub stats_file: PathBuf,
    pub courses_dir: PathBuf,
    pub failed_dir: PathBuf,
    pub tts_cache_dir: PathBuf,
}
```

and `from_root` includes `stats_file: root.join("stats.json"),`.

- [ ] **Step 2: Extend the existing `from_root_sets_mistakes_file` test to also assert `stats_file`**

Edit the test in `src/storage/paths.rs` (line ~79):

```rust
#[test]
fn from_root_sets_mistakes_and_stats_files() {
    let p = DataPaths::for_tests(PathBuf::from("/tmp/inkworm-test"));
    assert_eq!(
        p.mistakes_file,
        PathBuf::from("/tmp/inkworm-test/mistakes.json")
    );
    assert_eq!(
        p.stats_file,
        PathBuf::from("/tmp/inkworm-test/stats.json")
    );
}
```

(Rename the old test to avoid drift.)

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p inkworm --lib storage::paths::tests::from_root_sets_mistakes_and_stats_files`
Expected: FAIL (no `stats_file` field, or test ID changed).

Note: the lib name in `Cargo.toml` may be `inkworm` or unspecified; if `-p inkworm` fails, run `cargo test --lib from_root_sets_mistakes_and_stats_files` instead. This applies to every later `cargo test` invocation that filters by name.

- [ ] **Step 4: Now Step 1's edit should make it pass**

If Step 1 wasn't done first, do it now. Re-run the test.

Run: `cargo test --lib from_root_sets_mistakes_and_stats_files`
Expected: PASS.

- [ ] **Step 5: Declare `stats` submodule**

Edit `src/storage/mod.rs`. Add a line `pub mod stats;` alongside the other `pub mod` lines (alphabetical order: between `progress;` is fine to leave as last). Final layout:

```rust
pub mod atomic;
pub mod course;
pub mod failed;
pub mod icloud;
pub mod instance_lock;
pub mod migrate;
pub mod mistakes;
pub mod paths;
pub mod progress;
pub mod stats;
```

- [ ] **Step 6: Write failing tests for `Stats`**

Create `src/storage/stats.rs` with the test module first (TDD):

```rust
//! Per-day learning statistics: active study time, submits, accuracy, words.
//!
//! See spec: docs/superpowers/specs/2026-05-13-learning-stats-design.md

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::storage::atomic::write_atomic;
use crate::storage::course::StorageError;

pub const STATS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Stats {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(default)]
    pub days: BTreeMap<NaiveDate, DayStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DayStats {
    #[serde(rename = "activeMs", default)]
    pub active_ms: u64,
    #[serde(default)]
    pub submits: u32,
    #[serde(default)]
    pub correct: u32,
    #[serde(default)]
    pub words: u32,
}

impl Stats {
    pub fn empty() -> Self {
        Self {
            schema_version: STATS_SCHEMA_VERSION,
            days: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, StorageError> {
        // Mirror Progress::load: NotFound → empty, no exists()+read TOCTOU.
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => return Err(e.into()),
        };
        let mut s: Stats = serde_json::from_slice(&bytes)?;
        if s.schema_version == 0 {
            s.schema_version = STATS_SCHEMA_VERSION;
        }
        Ok(s)
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn empty_round_trips() {
        let s = Stats::empty();
        let json = serde_json::to_string(&s).unwrap();
        let s2: Stats = serde_json::from_str(&json).unwrap();
        assert_eq!(s, s2);
        assert_eq!(s2.schema_version, 1);
    }

    #[test]
    fn serde_uses_camel_case_keys() {
        let mut s = Stats::empty();
        let date = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
        s.days.insert(
            date,
            DayStats {
                active_ms: 12_345,
                submits: 24,
                correct: 22,
                words: 168,
            },
        );
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""schemaVersion":1"#));
        assert!(json.contains(r#""2026-05-13""#));
        assert!(json.contains(r#""activeMs":12345"#));
        assert!(json.contains(r#""submits":24"#));
    }

    #[test]
    fn load_missing_returns_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let s = Stats::load(&path).unwrap();
        assert_eq!(s, Stats::empty());
    }

    #[test]
    fn day_stats_default_is_zeroed() {
        let d = DayStats::default();
        assert_eq!(d.active_ms, 0);
        assert_eq!(d.submits, 0);
        assert_eq!(d.correct, 0);
        assert_eq!(d.words, 0);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("stats.json");
        let mut s = Stats::empty();
        let date = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
        s.days.insert(
            date,
            DayStats {
                active_ms: 743_000,
                submits: 24,
                correct: 22,
                words: 168,
            },
        );
        s.save(&path).unwrap();
        let s2 = Stats::load(&path).unwrap();
        assert_eq!(s, s2);
    }
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib storage::stats::tests`
Expected: 5 tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/storage/stats.rs src/storage/paths.rs src/storage/mod.rs
git commit -m "feat(stats): add Stats data model and stats.json path"
```

---

### Task 2: `StatsTracker` (pure in-memory accounting)

**Files:**
- Create: `src/stats/mod.rs`
- Create: `src/stats/tracker.rs`
- Modify: `src/lib.rs` (declare `pub mod stats;`)

- [ ] **Step 1: Declare the module in lib.rs**

Edit `src/lib.rs`. Add `pub mod stats;` alphabetically (between `judge` and `llm` is fine, since neither `judge` nor `llm` re-exports from `stats`):

```rust
pub mod app;
pub mod audio;
pub mod clock;
pub mod config;
pub mod error;
pub mod judge;
pub mod llm;
pub mod stats;
pub mod storage;
pub mod tts;
pub mod ui;
```

- [ ] **Step 2: Create stats/mod.rs skeleton**

Create `src/stats/mod.rs`:

```rust
//! Learning statistics — pure in-memory tracking and aggregation.
//!
//! See spec: docs/superpowers/specs/2026-05-13-learning-stats-design.md

pub mod tracker;

pub use tracker::{StatsTracker, IDLE_THRESHOLD_MS};
```

- [ ] **Step 3: Write failing tests for `StatsTracker`**

Create `src/stats/tracker.rs` with tests-first:

```rust
//! Pure in-memory active-time + submit accounting.
//!
//! Time accumulates only when a keystroke arrives within IDLE_THRESHOLD_MS
//! of the previous one. No background timer; no time advances unless an
//! event arrives.

use chrono::{DateTime, Local, NaiveDate, Utc};
use std::collections::BTreeMap;

use crate::storage::stats::{DayStats, Stats, STATS_SCHEMA_VERSION};

pub const IDLE_THRESHOLD_MS: i64 = 30_000;

pub struct StatsTracker {
    last_activity: Option<DateTime<Utc>>,
    today: NaiveDate,
    today_stats: DayStats,
    history: BTreeMap<NaiveDate, DayStats>,
}

impl StatsTracker {
    pub fn from_stats(mut stats: Stats, today: NaiveDate) -> Self {
        let today_stats = stats.days.remove(&today).unwrap_or_default();
        Self {
            last_activity: None,
            today,
            today_stats,
            history: stats.days,
        }
    }

    pub fn snapshot(&self) -> Stats {
        let mut days = self.history.clone();
        days.insert(self.today, self.today_stats);
        Stats {
            schema_version: STATS_SCHEMA_VERSION,
            days,
        }
    }

    pub fn today_stats(&self) -> &DayStats {
        &self.today_stats
    }

    pub fn on_keystroke(&mut self, now: DateTime<Utc>) {
        let today_local = now.with_timezone(&Local).date_naive();
        if today_local != self.today {
            // Rollover: move today into history, reset, drop session.
            self.history.insert(self.today, self.today_stats);
            self.today = today_local;
            self.today_stats = DayStats::default();
            self.last_activity = None;
        }
        if let Some(prev) = self.last_activity {
            if now >= prev {
                let delta_ms = now.signed_duration_since(prev).num_milliseconds();
                if delta_ms <= IDLE_THRESHOLD_MS {
                    self.today_stats.active_ms =
                        self.today_stats.active_ms.saturating_add(delta_ms as u64);
                }
                // delta > threshold or clock rewind → no accumulation.
            }
            // now < prev → clock rewind, no accumulation.
        }
        self.last_activity = Some(now);
    }

    pub fn on_submit(&mut self, now: DateTime<Utc>, was_correct: bool, words: u32) {
        self.on_keystroke(now);
        self.today_stats.submits = self.today_stats.submits.saturating_add(1);
        if was_correct {
            self.today_stats.correct = self.today_stats.correct.saturating_add(1);
        }
        self.today_stats.words = self.today_stats.words.saturating_add(words);
    }

    pub fn flush_idle(&mut self, now: DateTime<Utc>) {
        if let Some(prev) = self.last_activity {
            if now >= prev
                && now.signed_duration_since(prev).num_milliseconds() > IDLE_THRESHOLD_MS
            {
                self.last_activity = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(ymd: (i32, u32, u32), hms: (u32, u32, u32)) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(ymd.0, ymd.1, ymd.2, hms.0, hms.1, hms.2)
            .unwrap()
    }

    fn fresh() -> StatsTracker {
        let today = t((2026, 5, 13), (12, 0, 0))
            .with_timezone(&Local)
            .date_naive();
        StatsTracker::from_stats(Stats::empty(), today)
    }

    #[test]
    fn first_keystroke_records_zero_time() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        assert_eq!(tr.today_stats().active_ms, 0);
    }

    #[test]
    fn keystrokes_within_threshold_accumulate_delta() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 5))); // +5s
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 12))); // +7s
        assert_eq!(tr.today_stats().active_ms, 12_000);
    }

    #[test]
    fn gap_over_threshold_does_not_accumulate() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.on_keystroke(t((2026, 5, 13), (12, 1, 0))); // +60s, over 30s threshold
        assert_eq!(tr.today_stats().active_ms, 0);
        // Next pair within threshold accumulates again from the new anchor.
        tr.on_keystroke(t((2026, 5, 13), (12, 1, 10))); // +10s from 12:01:00
        assert_eq!(tr.today_stats().active_ms, 10_000);
    }

    #[test]
    fn submit_increments_correct_and_words() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.on_submit(t((2026, 5, 13), (12, 0, 3)), true, 5);
        let d = tr.today_stats();
        assert_eq!(d.submits, 1);
        assert_eq!(d.correct, 1);
        assert_eq!(d.words, 5);
        assert_eq!(d.active_ms, 3_000); // 12:00:00 → 12:00:03
    }

    #[test]
    fn submit_increments_total_even_when_wrong() {
        let mut tr = fresh();
        tr.on_submit(t((2026, 5, 13), (12, 0, 0)), false, 4);
        let d = tr.today_stats();
        assert_eq!(d.submits, 1);
        assert_eq!(d.correct, 0);
        assert_eq!(d.words, 4);
    }

    #[test]
    fn day_rollover_moves_today_to_history_and_resets() {
        let mut tr = fresh();
        tr.on_submit(t((2026, 5, 13), (23, 59, 0)), true, 3);
        // Crossing local midnight: pick a time well past local midnight in UTC
        // for any timezone the test runs in (UTC+14..UTC-12 worst cases).
        tr.on_keystroke(t((2026, 5, 14), (23, 0, 0))); // ~24h later
        let snap = tr.snapshot();
        assert!(snap.days.len() >= 1, "previous day should be in history");
        assert_eq!(tr.today_stats().submits, 0);
        assert_eq!(tr.today_stats().words, 0);
    }

    #[test]
    fn day_rollover_clears_last_activity() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (23, 59, 0)));
        // After rollover, a "first" keystroke on the new day must not accumulate
        // the multi-hour gap into the new day.
        tr.on_keystroke(t((2026, 5, 14), (23, 0, 0)));
        assert_eq!(tr.today_stats().active_ms, 0);
    }

    #[test]
    fn flush_idle_clears_session_after_threshold() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.flush_idle(t((2026, 5, 13), (12, 0, 45))); // > 30s
        // Next keystroke should NOT accumulate the gap.
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 50)));
        assert_eq!(tr.today_stats().active_ms, 0);
    }

    #[test]
    fn flush_idle_noop_when_within_threshold() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.flush_idle(t((2026, 5, 13), (12, 0, 20))); // < 30s
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 25))); // +25s from 12:00:00
        assert_eq!(tr.today_stats().active_ms, 25_000);
    }

    #[test]
    fn clock_rewind_does_not_subtract_time() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 10)));
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 5))); // earlier than prev
        assert_eq!(tr.today_stats().active_ms, 0);
    }

    #[test]
    fn snapshot_round_trips_through_stats() {
        let mut tr = fresh();
        tr.on_submit(t((2026, 5, 13), (12, 0, 0)), true, 3);
        tr.on_submit(t((2026, 5, 13), (12, 0, 2)), true, 4);
        let snap = tr.snapshot();
        let today = t((2026, 5, 13), (12, 0, 0))
            .with_timezone(&Local)
            .date_naive();
        let d = snap.days.get(&today).expect("today present");
        assert_eq!(d.submits, 2);
        assert_eq!(d.correct, 2);
        assert_eq!(d.words, 7);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib stats::tracker::tests`
Expected: 11 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/stats/mod.rs src/stats/tracker.rs
git commit -m "feat(stats): add StatsTracker with keystroke-idle active time"
```

---

### Task 3: `count_words` helper

**Files:**
- Modify: `src/stats/mod.rs`

- [ ] **Step 1: Write failing tests in stats/mod.rs**

Append to `src/stats/mod.rs`:

```rust
/// Counts English words in a drill reference string.
///
/// Whitespace-delimited. Empty / whitespace-only → 0.
pub fn count_words(english: &str) -> u32 {
    english.split_whitespace().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_words_empty_is_zero() {
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn count_words_whitespace_only_is_zero() {
        assert_eq!(count_words("   \t  "), 0);
    }

    #[test]
    fn count_words_single() {
        assert_eq!(count_words("hello"), 1);
    }

    #[test]
    fn count_words_trims_and_collapses() {
        assert_eq!(count_words("  hello   world  "), 2);
    }

    #[test]
    fn count_words_punctuation_does_not_split() {
        // "AI's" is one whitespace-delimited token, even though it contains
        // punctuation; this matches "I counted 5 words".
        assert_eq!(count_words("AI's mission is to learn fast"), 6);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib stats::tests`
Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/stats/mod.rs
git commit -m "feat(stats): add count_words helper"
```

---

### Task 4: Aggregation functions (pure)

**Files:**
- Create: `src/stats/aggregate.rs`
- Modify: `src/stats/mod.rs` (declare module + re-exports)

- [ ] **Step 1: Add `pub mod aggregate;` and re-exports**

Edit `src/stats/mod.rs` to add `pub mod aggregate;` after `pub mod tracker;`, and update the `pub use` line:

```rust
pub mod aggregate;
pub mod tracker;

pub use aggregate::{
    all_time_totals, recent_months, recent_weeks, this_week, today_totals, week_totals,
    MonthRow, Totals, WeekDayCell, WeekRow,
};
pub use tracker::{StatsTracker, IDLE_THRESHOLD_MS};
```

- [ ] **Step 2: Write `aggregate.rs` with tests-first**

Create `src/stats/aggregate.rs`:

```rust
//! Pure read-only aggregations over `Stats`.
//!
//! Inputs are `&Stats` plus a "today" `NaiveDate` anchor. No IO, no state,
//! no caching. Accuracy is never stored — render-time function of totals.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::storage::stats::{DayStats, Stats};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub active_ms: u64,
    pub submits: u32,
    pub correct: u32,
    pub words: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekDayCell {
    pub date: NaiveDate,
    pub stats: DayStats,
    pub is_today: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekRow {
    pub iso_year: i32,
    pub iso_week: u32,
    pub totals: Totals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthRow {
    pub year: i32,
    pub month: u32,
    pub totals: Totals,
}

fn merge(into: &mut Totals, d: &DayStats) {
    into.active_ms = into.active_ms.saturating_add(d.active_ms);
    into.submits = into.submits.saturating_add(d.submits);
    into.correct = into.correct.saturating_add(d.correct);
    into.words = into.words.saturating_add(d.words);
}

fn monday_of(d: NaiveDate) -> NaiveDate {
    let weekday = d.weekday().num_days_from_monday() as i64;
    d - Duration::days(weekday)
}

pub fn today_totals(stats: &Stats, today: NaiveDate) -> Totals {
    stats
        .days
        .get(&today)
        .map(|d| {
            let mut t = Totals::default();
            merge(&mut t, d);
            t
        })
        .unwrap_or_default()
}

pub fn week_totals(stats: &Stats, today: NaiveDate) -> Totals {
    let mon = monday_of(today);
    let sun = mon + Duration::days(6);
    let mut t = Totals::default();
    for (d, ds) in stats.days.range(mon..=sun) {
        let _ = d;
        merge(&mut t, ds);
    }
    t
}

pub fn all_time_totals(stats: &Stats) -> Totals {
    let mut t = Totals::default();
    for ds in stats.days.values() {
        merge(&mut t, ds);
    }
    t
}

pub fn this_week(stats: &Stats, today: NaiveDate) -> [WeekDayCell; 7] {
    let mon = monday_of(today);
    let mut out = [WeekDayCell {
        date: mon,
        stats: DayStats::default(),
        is_today: false,
    }; 7];
    for (i, cell) in out.iter_mut().enumerate() {
        let date = mon + Duration::days(i as i64);
        cell.date = date;
        cell.stats = stats.days.get(&date).copied().unwrap_or_default();
        cell.is_today = date == today;
    }
    out
}

/// Returns up to `n` past ISO weeks (newest first), **excluding** the
/// current ISO week (which is already presented as the "This week" strip).
/// Empty weeks are padded with zero `Totals` to keep the row count stable.
pub fn recent_weeks(stats: &Stats, today: NaiveDate, n: usize) -> Vec<WeekRow> {
    let mut out = Vec::with_capacity(n);
    let current_mon = monday_of(today);
    for i in 1..=n {
        let mon = current_mon - Duration::days(7 * i as i64);
        let sun = mon + Duration::days(6);
        let mut t = Totals::default();
        for (_d, ds) in stats.days.range(mon..=sun) {
            merge(&mut t, ds);
        }
        let iso = mon.iso_week();
        out.push(WeekRow {
            iso_year: iso.year(),
            iso_week: iso.week(),
            totals: t,
        });
    }
    out
}

/// Returns up to `n` past months (newest first), **including** the current
/// month. Empty months are padded with zero `Totals`.
pub fn recent_months(stats: &Stats, today: NaiveDate, n: usize) -> Vec<MonthRow> {
    let mut out = Vec::with_capacity(n);
    let (mut year, mut month) = (today.year(), today.month());
    for _ in 0..n {
        let start = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        let end = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
        };
        let mut t = Totals::default();
        for (_d, ds) in stats.days.range(start..end) {
            merge(&mut t, ds);
        }
        out.push(MonthRow {
            year,
            month,
            totals: t,
        });
        // step back one month
        if month == 1 {
            month = 12;
            year -= 1;
        } else {
            month -= 1;
        }
    }
    out
}

/// Returns the Monday of the given date's ISO week. Public so UI can show
/// "This week (ISO 2026-W20)" in the header.
pub fn iso_week_label(d: NaiveDate) -> (i32, u32) {
    let iso = d.iso_week();
    (iso.year(), iso.week())
}

// keep this re-exportable
pub use chrono::Weekday as _Weekday;

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn ds(active_ms: u64, submits: u32, correct: u32, words: u32) -> DayStats {
        DayStats {
            active_ms,
            submits,
            correct,
            words,
        }
    }

    fn stats_with(entries: &[(NaiveDate, DayStats)]) -> Stats {
        let mut s = Stats::empty();
        for (d, ds) in entries {
            s.days.insert(*d, *ds);
        }
        s
    }

    #[test]
    fn today_totals_picks_correct_day() {
        let s = stats_with(&[
            (day(2026, 5, 12), ds(1000, 2, 2, 5)),
            (day(2026, 5, 13), ds(2000, 3, 2, 9)),
        ]);
        let t = today_totals(&s, day(2026, 5, 13));
        assert_eq!(t, Totals { active_ms: 2000, submits: 3, correct: 2, words: 9 });
    }

    #[test]
    fn today_totals_returns_zero_for_empty_stats() {
        let s = Stats::empty();
        assert_eq!(today_totals(&s, day(2026, 5, 13)), Totals::default());
    }

    #[test]
    fn week_totals_sums_mon_through_sun() {
        // 2026-05-13 is a Wednesday → week is 2026-05-11 (Mon) .. 2026-05-17 (Sun).
        let s = stats_with(&[
            (day(2026, 5, 10), ds(999, 9, 9, 9)), // Sun before — excluded
            (day(2026, 5, 11), ds(1000, 1, 1, 5)),
            (day(2026, 5, 13), ds(2000, 2, 1, 8)),
            (day(2026, 5, 17), ds(3000, 1, 0, 4)),
            (day(2026, 5, 18), ds(777, 7, 7, 7)), // Mon after — excluded
        ]);
        let t = week_totals(&s, day(2026, 5, 13));
        assert_eq!(t, Totals { active_ms: 6000, submits: 4, correct: 2, words: 17 });
    }

    #[test]
    fn this_week_marks_today_cell() {
        let s = stats_with(&[(day(2026, 5, 13), ds(1, 1, 1, 1))]);
        let cells = this_week(&s, day(2026, 5, 13));
        assert_eq!(cells.len(), 7);
        assert_eq!(cells[0].date, day(2026, 5, 11)); // Mon
        assert_eq!(cells[6].date, day(2026, 5, 17)); // Sun
        assert!(!cells[0].is_today);
        assert!(cells[2].is_today); // Wed
        assert_eq!(cells[2].stats.submits, 1);
    }

    #[test]
    fn this_week_handles_year_boundary() {
        // 2026-01-01 is a Thursday → Mon = 2025-12-29.
        let cells = this_week(&Stats::empty(), day(2026, 1, 1));
        assert_eq!(cells[0].date, day(2025, 12, 29));
        assert_eq!(cells[6].date, day(2026, 1, 4));
        assert!(cells[3].is_today); // Thu
    }

    #[test]
    fn recent_weeks_orders_newest_first_and_pads_empty() {
        let s = stats_with(&[(day(2026, 4, 27), ds(100, 1, 1, 1))]); // ISO 2026-W18 (Mon)
        let rows = recent_weeks(&s, day(2026, 5, 13), 4);
        assert_eq!(rows.len(), 4);
        // newest first = the week immediately before the current one
        let weeks: Vec<_> = rows.iter().map(|r| (r.iso_year, r.iso_week)).collect();
        assert_eq!(weeks[0], (2026, 19)); // current is W20, so newest excluded is W19
        assert_eq!(weeks[3], (2026, 16));
        // 2026-04-27 is in W18 → rows[1].totals carries it
        assert_eq!(rows[1].totals.active_ms, 100);
        assert_eq!(rows[0].totals, Totals::default());
    }

    #[test]
    fn recent_weeks_excludes_current_iso_week() {
        let s = stats_with(&[(day(2026, 5, 13), ds(9_999, 9, 9, 9))]); // in current week
        let rows = recent_weeks(&s, day(2026, 5, 13), 1);
        assert_eq!(rows[0].totals, Totals::default());
    }

    #[test]
    fn recent_months_includes_current_month() {
        let s = stats_with(&[(day(2026, 5, 13), ds(1234, 5, 4, 20))]);
        let rows = recent_months(&s, day(2026, 5, 13), 3);
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].year, rows[0].month), (2026, 5));
        assert_eq!((rows[1].year, rows[1].month), (2026, 4));
        assert_eq!((rows[2].year, rows[2].month), (2026, 3));
        assert_eq!(rows[0].totals.active_ms, 1234);
        assert_eq!(rows[1].totals, Totals::default());
    }

    #[test]
    fn recent_months_handles_year_rollover() {
        let rows = recent_months(&Stats::empty(), day(2026, 2, 5), 4);
        let months: Vec<_> = rows.iter().map(|r| (r.year, r.month)).collect();
        assert_eq!(months, vec![(2026, 2), (2026, 1), (2025, 12), (2025, 11)]);
    }

    #[test]
    fn all_time_totals_sums_every_day() {
        let s = stats_with(&[
            (day(2025, 1, 1), ds(1, 1, 1, 1)),
            (day(2026, 5, 13), ds(2, 2, 2, 2)),
        ]);
        let t = all_time_totals(&s);
        assert_eq!(t, Totals { active_ms: 3, submits: 3, correct: 3, words: 3 });
    }
}
```

(`_Weekday` re-export is there because some compilers need it surfaced when downstream tests reference `Weekday` paths — keep it; harmless.)

- [ ] **Step 3: Run tests**

Run: `cargo test --lib stats::aggregate::tests`
Expected: 10 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/stats/aggregate.rs src/stats/mod.rs
git commit -m "feat(stats): add pure aggregations (totals, weekly, monthly)"
```

---

### Task 5: Extend `StudyState::submit` to emit a `SubmitTick`

**Files:**
- Modify: `src/ui/study.rs` (signature + tests already in the file)
- Modify: `src/app.rs` (call sites — `handle_study_key` only currently)

- [ ] **Step 1: Add `SubmitTick` and update existing tests in `src/ui/study.rs`**

Add the type after `SubmitOutcome` (around line 45):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubmitTick {
    pub was_correct: bool,
    pub words: u32,
}
```

Then update the signature of `submit` (line 217) to return a tuple. Locate `pub fn submit(&mut self, clock: &dyn Clock) -> Option<SubmitOutcome>` and replace:

```rust
pub fn submit(&mut self, clock: &dyn Clock) -> (Option<SubmitOutcome>, Option<SubmitTick>) {
    if self.phase != StudyPhase::Active {
        return (None, None);
    }
    if self.feedback != FeedbackState::Typing {
        return (None, None);
    }
    let course = match self.course.as_ref() {
        Some(c) => c,
        None => return (None, None),
    };
    let sentence = match course.sentences.get(self.sentence_idx) {
        Some(s) => s,
        None => return (None, None),
    };
    let drill = match sentence.drills.get(self.drill_idx) {
        Some(d) => d,
        None => return (None, None),
    };
    let was_correct = judge::equals(&self.input, &drill.english);
    let words = drill.english.split_whitespace().count() as u32;
    let drill_ref = DrillRef {
        course_id: course.id.clone(),
        sentence_order: sentence.order,
        drill_stage: drill.stage,
    };
    let outcome = if self.first_attempt_pending {
        self.first_attempt_pending = false;
        Some(SubmitOutcome {
            drill_ref,
            first_attempt_correct: was_correct,
        })
    } else {
        None
    };
    let tick = Some(SubmitTick { was_correct, words });
    if was_correct {
        if matches!(self.mode, StudyMode::Course) {
            self.record_correct(clock);
        }
        self.feedback = FeedbackState::Correct;
        self.correct_at = Some(clock.now());
    } else {
        self.feedback = FeedbackState::Wrong;
    }
    (outcome, tick)
}
```

The early returns must use the tuple — every existing `return None` becomes `return (None, None)`.

Also note that the existing mistakes-mode branch in this file (around line 829, inside a separate code path that constructs `SubmitOutcome` directly) must keep working — verify by re-running tests after the change. If any caller in `src/ui/study.rs` unpacks the old `Option<SubmitOutcome>` shape, fix to the tuple.

- [ ] **Step 2: Update existing tests in study.rs that call submit**

Find the existing tests that assert on `submit(...)` (in `src/ui/study.rs` around line 924 — `submit_first_attempt_correct_returns_true_outcome_and_marks_correct` and any siblings). Adjust each unpack site:

```rust
let (outcome, tick) = state.submit(&clk);
assert_eq!(outcome, Some(SubmitOutcome { ... }));
assert_eq!(tick, Some(SubmitTick { was_correct: true, words: ... }));
```

For every existing `state.submit(&clk);` standalone (return value discarded), no edit is needed — Rust still accepts an ignored tuple.

For every existing `state.submit(&clk)` whose returned `Option<SubmitOutcome>` is matched (e.g., `let outcome = state.submit(&clk); if let Some(o) = outcome { ... }`), refactor to `let (outcome, _) = state.submit(&clk); if let Some(o) = outcome { ... }`.

Search before editing:

```
rg -n 'state\.submit\(' src/ui/study.rs tests/
```

Update each hit consistently.

- [ ] **Step 3: Add a fresh test for `SubmitTick` semantics**

Append in the `#[cfg(test)] mod tests` block of `src/ui/study.rs`:

```rust
#[test]
fn submit_returns_tick_for_every_attempt() {
    use chrono::{TimeZone, Utc};
    let clk = crate::clock::FixedClock(Utc.with_ymd_and_hms(2026, 5, 13, 12, 0, 0).unwrap());
    let course = crate::storage::course::Course {
        schema_version: 2,
        id: "2026-05-13-test".into(),
        title: "T".into(),
        description: None,
        source: crate::storage::course::Source {
            kind: crate::storage::course::SourceKind::Manual,
            url: String::new(),
            created_at: Utc.with_ymd_and_hms(2026, 5, 13, 0, 0, 0).unwrap(),
            model: String::new(),
        },
        sentences: vec![crate::storage::course::Sentence {
            order: 1,
            drills: vec![crate::storage::course::Drill {
                stage: 1,
                focus: crate::storage::course::Focus::Keywords,
                chinese: "你好".into(),
                english: "hello world".into(),
                soundmark: String::new(),
            }],
        }],
    };
    let mut s = StudyState::new(Some(course), Progress::empty());
    // Wrong attempt
    for c in "wrong".chars() { s.type_char(c); }
    let (out1, tick1) = s.submit(&clk);
    assert!(out1.is_some()); // first attempt
    assert_eq!(tick1, Some(SubmitTick { was_correct: false, words: 2 }));
    // After Wrong, clear input
    s.clear_and_restart();
    for c in "hello world".chars() { s.type_char(c); }
    let (out2, tick2) = s.submit(&clk);
    assert!(out2.is_none()); // first_attempt_pending consumed
    assert_eq!(tick2, Some(SubmitTick { was_correct: true, words: 2 }));
}
```

(Adjust imports at the top of the test module if `Progress` isn't already in scope.)

- [ ] **Step 4: Update app.rs call site**

Edit `src/app.rs::handle_study_key`. Line ~746 currently reads:

```rust
let outcome = self.study.submit(self.clock.as_ref());
if let Some(o) = outcome {
    self.handle_submit_outcome(o);
}
```

Replace with:

```rust
let (outcome, tick) = self.study.submit(self.clock.as_ref());
if let Some(o) = outcome {
    self.handle_submit_outcome(o);
}
let _ = tick; // wired up in Task 6
```

(Keeping `tick` bound but unused with `let _ = tick;` makes Task 6's wiring a one-line change.)

- [ ] **Step 5: Run the test suite**

Run: `cargo test`
Expected: PASS. If any test breaks, it's a call-site that needs the tuple unpack — fix and re-run.

- [ ] **Step 6: Commit**

```bash
git add src/ui/study.rs src/app.rs
git commit -m "refactor(study): submit() emits SubmitTick per attempt"
```

---

### Task 6: App integration — load, hooks, save

**Files:**
- Modify: `src/app.rs` (add `stats` field, wire hooks, save on submit & quit)

- [ ] **Step 1: Add the field and imports**

Edit `src/app.rs` near the top imports:

```rust
use crate::stats::{count_words, StatsTracker};
use crate::storage::stats::Stats;
```

Add to `pub struct App` after `last_seen_day` (line ~65):

```rust
    stats: StatsTracker,
```

- [ ] **Step 2: Construct in `App::new`**

Edit `src/app.rs::App::new` — find where `last_seen_day = clock.today_local();` is computed (line ~97). After that line, before the `let mut app = Self {`:

```rust
        let stats_loaded = Stats::load(&data_paths.stats_file).unwrap_or_else(|e| {
            tracing::warn!("Failed to load stats: {e}");
            Stats::empty()
        });
        let stats = StatsTracker::from_stats(stats_loaded, last_seen_day);
```

Then add `stats,` inside the `Self { ... }` literal.

- [ ] **Step 3: Install the keystroke hook**

Edit `src/app.rs::handle_study_key`. The whole body should fire `on_keystroke` once per real keystroke event (one event = one accumulation tick). Add **right at the top of the function** (before the info_banner clearing, line ~691):

```rust
        self.stats.on_keystroke(self.clock.now());
```

Rationale: even a key that resets feedback or dismisses a banner is still a "user is here, typing" signal.

- [ ] **Step 4: Install the submit hook**

Edit the submit branch added in Task 5 Step 4. Replace `let _ = tick;` with:

```rust
        if let Some(t) = tick {
            self.stats.on_submit(self.clock.now(), t.was_correct, t.words);
            let snap = self.stats.snapshot();
            if let Err(e) = snap.save(&self.data_paths.stats_file) {
                tracing::warn!("Failed to save stats: {e}");
                self.info_banner = Some(format!("Failed to save stats: {e}"));
            }
        }
```

The `on_keystroke` you added in Step 3 already accounts for this event's time; `on_submit` calls `on_keystroke` internally for safety (double-call is idempotent: the second is a 0-delta no-op because `last_activity == now`).

- [ ] **Step 5: Install the tick hook**

Edit `src/app.rs::on_tick` (line ~470). Right after `self.blink_counter += 1;` (so it fires every tick regardless of screen):

```rust
        self.stats.flush_idle(self.clock.now());
```

- [ ] **Step 6: Install the quit save**

Find `fn quit` (line ~1125):

```rust
    fn quit(&mut self) {
        // ... existing body ...
    }
```

At the **start** of the body, add:

```rust
        let snap = self.stats.snapshot();
        if let Err(e) = snap.save(&self.data_paths.stats_file) {
            tracing::warn!("Failed to save stats on quit: {e}");
        }
```

(Errors are logged but don't block quit.)

- [ ] **Step 7: Write an integration test**

Create `tests/stats.rs`:

```rust
mod common;

use chrono::{TimeZone, Utc};
use inkworm::clock::FixedClock;
use inkworm::stats::StatsTracker;
use inkworm::storage::progress::Progress;
use inkworm::storage::stats::Stats;
use inkworm::ui::study::{StudyState, SubmitTick};
use std::sync::Arc;

fn at(h: u32, m: u32, s: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 13, h, m, s).unwrap()
}

#[test]
fn retry_until_correct_counts_each_submit() {
    let course = common::load_minimal_course();
    let english = course.sentences[0].drills[0].english.clone();
    let words = english.split_whitespace().count() as u32;

    let today = at(12, 0, 0).with_timezone(&chrono::Local).date_naive();
    let mut tr = StatsTracker::from_stats(Stats::empty(), today);

    let mut s = StudyState::new(Some(course), Progress::empty());
    let clk = FixedClock(at(12, 0, 0));

    // Wrong attempt
    for c in "wrong".chars() { s.type_char(c); }
    let (_o, tick) = s.submit(&clk);
    let t = tick.expect("tick present");
    tr.on_submit(at(12, 0, 0), t.was_correct, t.words);
    s.clear_and_restart();

    // Correct attempt
    for c in english.chars() { s.type_char(c); }
    let clk2 = FixedClock(at(12, 0, 5));
    let (_o2, tick2) = s.submit(&clk2);
    let t2 = tick2.expect("tick present");
    tr.on_submit(at(12, 0, 5), t2.was_correct, t2.words);

    let d = tr.today_stats();
    assert_eq!(d.submits, 2, "both attempts counted");
    assert_eq!(d.correct, 1, "only the second counted correct");
    assert_eq!(d.words, words * 2);
}

#[test]
fn submit_writes_stats_file_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stats.json");
    let today = chrono::Local::now().date_naive();
    let mut tr = StatsTracker::from_stats(Stats::empty(), today);
    tr.on_submit(Utc::now(), true, 3);
    let _ = Arc::new(()); // (linker keeps Arc in scope)
    tr.snapshot().save(&path).unwrap();
    let reloaded = Stats::load(&path).unwrap();
    assert!(reloaded.days.contains_key(&today));
}
```

(`tests/common/mod.rs` already exists with `load_minimal_course` — see `src/storage/paths.rs::for_tests` and the existing `tests/ui.rs::common`.)

- [ ] **Step 8: Run tests**

Run: `cargo test --test stats`
Expected: 2 tests pass.

Also run the full suite to catch regressions:

Run: `cargo test`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs tests/stats.rs
git commit -m "feat(stats): wire StatsTracker into App with submit/idle/quit hooks"
```

---

### Task 7: Palette `/stats` command + `Screen::Stats` skeleton

**Files:**
- Modify: `src/ui/palette.rs` (add command entry)
- Modify: `src/app.rs` (Screen variant, dispatch, execute_command branch)
- Create: `src/ui/stats.rs` (minimal empty-state render)
- Modify: `src/ui/mod.rs` (declare module)

- [ ] **Step 1: Declare the UI module**

Edit `src/ui/mod.rs`. Add `pub mod stats;` alphabetically alongside the other `pub mod` lines.

- [ ] **Step 2: Add the palette command**

Edit `src/ui/palette.rs::COMMANDS`. After the `mistakes` entry (around line 81):

```rust
    Command {
        name: "stats",
        aliases: &[],
        description: "View learning statistics",
        available: true,
        takes_args: false,
    },
```

- [ ] **Step 3: Add the `Stats` Screen variant + render dispatch**

Edit `src/app.rs::Screen` (line 26):

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
    Doctor,
    Stats,
}
```

Edit the `on_input` `match &self.screen` (around line 524) — add:

```rust
                Screen::Stats => {
                    if key.code == KeyCode::Esc {
                        self.screen = Screen::Study;
                    } else if key.code == KeyCode::Char('m') {
                        self.stats_view = StatsView::Monthly;
                    } else if key.code == KeyCode::Char('w') {
                        self.stats_view = StatsView::Weekly;
                    }
                }
```

Add field `stats_view: StatsView` to the `App` struct (after `stats`):

```rust
    stats_view: StatsView,
```

Define the enum at the top of `src/app.rs` (after `Screen`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsView {
    Weekly,
    Monthly,
}
```

Initialize in `App::new`:

```rust
            stats_view: StatsView::Weekly,
```

Edit `App::render` (line ~1237) — add a new arm:

```rust
            Screen::Stats => {
                let inner = self.render_chrome(frame);
                crate::ui::stats::render_stats(
                    frame,
                    inner,
                    &self.stats.snapshot(),
                    self.clock.today_local(),
                    self.stats_view,
                );
            }
```

Edit `App::execute_command` (line ~1031) — add to the match:

```rust
            "stats" => self.screen = Screen::Stats,
```

- [ ] **Step 4: Create minimal `render_stats` (empty-state version)**

Create `src/ui/stats.rs`:

```rust
//! Full-screen statistics view (`/stats`).
//!
//! Layout: three summary cards (Today / This week / All time), a 7-row
//! "This week" day strip with bars, and either a "Previous 12 weeks" or
//! "Last 12 months" table depending on the view mode.

use chrono::NaiveDate;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Paragraph,
    Frame,
};

use crate::app::StatsView;
use crate::storage::stats::Stats;

pub fn render_stats(
    frame: &mut Frame,
    area: Rect,
    stats: &Stats,
    today: NaiveDate,
    view: StatsView,
) {
    if stats.days.is_empty() {
        let msg = Paragraph::new("No data yet — start studying with /go")
            .style(Style::default().fg(Color::DarkGray))
            .centered();
        let y = area.height / 2;
        frame.render_widget(msg, Rect::new(area.x, area.y + y, area.width, 1));
        // hint
        let hint = Paragraph::new(Line::from("esc back"))
            .style(Style::default().fg(Color::DarkGray))
            .centered();
        frame.render_widget(
            hint,
            Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1),
        );
        let _ = (today, view); // suppress unused-warning until Task 8 populates them
        return;
    }
    // Full render lives in Task 8; for the skeleton, defer to a placeholder.
    let placeholder = Paragraph::new("Stats view — full render coming in Task 8")
        .style(Style::default().fg(Color::DarkGray))
        .centered();
    frame.render_widget(placeholder, Rect::new(area.x, area.y, area.width, 1));
    let _ = (today, view);
}
```

- [ ] **Step 5: Write a palette test**

Create `tests/stats_palette.rs`:

```rust
use inkworm::ui::palette::{PaletteState, COMMANDS};

#[test]
fn stats_is_a_registered_command() {
    assert!(COMMANDS.iter().any(|c| c.name == "stats"));
}

#[test]
fn slash_stats_parses_to_stats_command() {
    let mut p = PaletteState::new();
    p.input = "/stats".into();
    let (cmd, args) = p.parse();
    assert_eq!(cmd, "stats");
    assert!(args.is_empty());
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --test stats_palette`
Expected: 2 tests pass.

Full suite:

Run: `cargo test`
Expected: PASS. If `Screen::Stats` is referenced anywhere else (e.g., an exhaustive match), the compiler will tell you — add the arm.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/ui/palette.rs src/ui/mod.rs src/ui/stats.rs tests/stats_palette.rs
git commit -m "feat(stats): add /stats palette command and Screen scaffolding"
```

---

### Task 8: Full `render_stats` — cards, weekly strip, weekly + monthly tables

**Files:**
- Modify: `src/ui/stats.rs` (replace placeholder body with full render)

- [ ] **Step 1: Replace `render_stats` with the full version**

Open `src/ui/stats.rs` and replace the function (and any helper imports) with:

```rust
//! Full-screen statistics view (`/stats`).

use chrono::NaiveDate;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::StatsView;
use crate::stats::{
    aggregate::iso_week_label, all_time_totals, recent_months, recent_weeks, this_week,
    today_totals, week_totals, Totals,
};
use crate::storage::stats::Stats;

pub fn render_stats(
    frame: &mut Frame,
    area: Rect,
    stats: &Stats,
    today: NaiveDate,
    view: StatsView,
) {
    if stats.days.is_empty() {
        render_empty(frame, area);
        return;
    }

    // Vertical layout: 3 cards (3 rows), week strip (10 rows including
    // header + 7 days + blank + section title), recent table (rest), hint.
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // cards
            Constraint::Length(10), // weekly strip
            Constraint::Min(3),     // recent table
            Constraint::Length(1),  // hint
        ])
        .split(area);

    render_cards(frame, chunks[0], stats, today);
    render_weekly_strip(frame, chunks[1], stats, today);
    match view {
        StatsView::Weekly => render_recent_weeks_table(frame, chunks[2], stats, today),
        StatsView::Monthly => render_recent_months_table(frame, chunks[2], stats, today),
    }
    let hint_text = match view {
        StatsView::Weekly => "esc back   m: monthly view",
        StatsView::Monthly => "esc back   w: weekly view",
    };
    let hint = Paragraph::new(Line::from(hint_text))
        .style(Style::default().fg(Color::DarkGray))
        .centered();
    frame.render_widget(hint, chunks[3]);
}

fn render_empty(frame: &mut Frame, area: Rect) {
    let msg = Paragraph::new("No data yet — start studying with /go")
        .style(Style::default().fg(Color::DarkGray))
        .centered();
    let y = area.height / 2;
    frame.render_widget(msg, Rect::new(area.x, area.y + y, area.width, 1));
    let hint = Paragraph::new(Line::from("esc back"))
        .style(Style::default().fg(Color::DarkGray))
        .centered();
    frame.render_widget(
        hint,
        Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1),
    );
}

fn render_cards(frame: &mut Frame, area: Rect, stats: &Stats, today: NaiveDate) {
    let today_t = today_totals(stats, today);
    let week_t = week_totals(stats, today);
    let all_t = all_time_totals(stats);
    let rows = [
        ("Today      ", today_t),
        ("This week  ", week_t),
        ("All time   ", all_t),
    ];
    for (i, (label, t)) in rows.iter().enumerate() {
        let line = Line::from(vec![
            Span::styled((*label).to_string(), Style::default().fg(Color::Gray)),
            Span::raw(format!(
                "{:<10} {:>5} submits   {:>5} correct ({})  {:>6}w",
                fmt_duration(t.active_ms),
                t.submits,
                t.correct,
                fmt_accuracy(t),
                t.words
            )),
        ]);
        let p = Paragraph::new(line);
        let y = area.y.saturating_add(i as u16);
        if y >= area.y + area.height {
            break;
        }
        frame.render_widget(p, Rect::new(area.x, y, area.width, 1));
    }
}

fn render_weekly_strip(frame: &mut Frame, area: Rect, stats: &Stats, today: NaiveDate) {
    let (iso_year, iso_week) = iso_week_label(today);
    let title = Paragraph::new(format!(
        "This week (Mon–Sun, ISO {}-W{:02})",
        iso_year, iso_week
    ))
    .style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(title, Rect::new(area.x, area.y, area.width, 1));

    let cells = this_week(stats, today);
    let max_ms = cells.iter().map(|c| c.stats.active_ms).max().unwrap_or(0);
    let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    for (i, cell) in cells.iter().enumerate() {
        let y = area.y.saturating_add(2 + i as u16);
        if y >= area.y + area.height {
            break;
        }
        let bar = bar_for(cell.stats.active_ms, max_ms, 10);
        let has_data = cell.stats.submits > 0 || cell.stats.active_ms > 0;
        let body = if has_data {
            format!(
                "  {} {}  {}  {:>4} {}/{} {} {:>4}w",
                weekdays[i],
                cell.date.format("%m-%d"),
                bar,
                fmt_duration(cell.stats.active_ms),
                cell.stats.correct,
                cell.stats.submits,
                fmt_accuracy_short(&Totals {
                    active_ms: cell.stats.active_ms,
                    submits: cell.stats.submits,
                    correct: cell.stats.correct,
                    words: cell.stats.words,
                }),
                cell.stats.words
            )
        } else {
            format!(
                "  {} {}  {}  --",
                weekdays[i],
                cell.date.format("%m-%d"),
                bar_for(0, max_ms.max(1), 10)
            )
        };
        let mut spans = vec![Span::raw(body)];
        if cell.is_today {
            spans.push(Span::styled(
                "  ← today",
                Style::default().fg(Color::Yellow),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), Rect::new(area.x, y, area.width, 1));
    }
}

fn render_recent_weeks_table(frame: &mut Frame, area: Rect, stats: &Stats, today: NaiveDate) {
    let rows = recent_weeks(stats, today, 12);
    let header = Paragraph::new(Line::from(Span::styled(
        "Previous 12 weeks (excludes current week)",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(header, Rect::new(area.x, area.y, area.width, 1));
    let cols = Paragraph::new(Line::from(Span::styled(
        "  ISO Week    Time     Submits   Acc    Words",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(cols, Rect::new(area.x, area.y.saturating_add(1), area.width, 1));
    for (i, row) in rows.iter().enumerate() {
        let y = area.y.saturating_add(2 + i as u16);
        if y >= area.y + area.height {
            break;
        }
        let body = format!(
            "  {}-W{:02}   {:>6}   {:>6}    {:>4}   {:>5}",
            row.iso_year,
            row.iso_week,
            fmt_duration(row.totals.active_ms),
            row.totals.submits,
            fmt_accuracy_short(&row.totals),
            row.totals.words,
        );
        frame.render_widget(Paragraph::new(body), Rect::new(area.x, y, area.width, 1));
    }
}

fn render_recent_months_table(frame: &mut Frame, area: Rect, stats: &Stats, today: NaiveDate) {
    let rows = recent_months(stats, today, 12);
    let header = Paragraph::new(Line::from(Span::styled(
        "Last 12 months (includes current month)",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(header, Rect::new(area.x, area.y, area.width, 1));
    let cols = Paragraph::new(Line::from(Span::styled(
        "  Month      Time     Submits   Acc    Words",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(cols, Rect::new(area.x, area.y.saturating_add(1), area.width, 1));
    for (i, row) in rows.iter().enumerate() {
        let y = area.y.saturating_add(2 + i as u16);
        if y >= area.y + area.height {
            break;
        }
        let body = format!(
            "  {}-{:02}    {:>6}   {:>6}    {:>4}   {:>5}",
            row.year,
            row.month,
            fmt_duration(row.totals.active_ms),
            row.totals.submits,
            fmt_accuracy_short(&row.totals),
            row.totals.words,
        );
        frame.render_widget(Paragraph::new(body), Rect::new(area.x, y, area.width, 1));
    }
}

/// "0m" / "Xm" / "Xh YYm". Always at most 6 chars wide.
pub(crate) fn fmt_duration(ms: u64) -> String {
    let secs = ms / 1000;
    let mins = secs / 60;
    if mins < 1 {
        "0m".to_string()
    } else if mins < 60 {
        format!("{}m", mins)
    } else {
        format!("{}h {:02}m", mins / 60, mins % 60)
    }
}

pub(crate) fn fmt_accuracy(t: &Totals) -> String {
    if t.submits == 0 {
        "--".to_string()
    } else {
        let pct = (t.correct as f64 * 100.0 / t.submits as f64).round() as u32;
        format!("{}%", pct)
    }
}

pub(crate) fn fmt_accuracy_short(t: &Totals) -> String {
    if t.submits == 0 {
        "--".to_string()
    } else {
        let pct = (t.correct as f64 * 100.0 / t.submits as f64).round() as u32;
        format!("{}%", pct)
    }
}

fn bar_for(value: u64, max: u64, width: usize) -> String {
    if max == 0 {
        return "░".repeat(width);
    }
    let filled = ((value as f64 / max as f64) * width as f64).round() as usize;
    let filled = filled.min(width);
    let mut s = String::with_capacity(width);
    for _ in 0..filled {
        s.push('█');
    }
    for _ in filled..width {
        s.push('░');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::stats::DayStats;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn t(active_ms: u64, submits: u32, correct: u32, words: u32) -> DayStats {
        DayStats { active_ms, submits, correct, words }
    }

    #[test]
    fn fmt_duration_handles_zero_minutes() {
        assert_eq!(fmt_duration(0), "0m");
        assert_eq!(fmt_duration(30_000), "0m");
        assert_eq!(fmt_duration(60_000), "1m");
        assert_eq!(fmt_duration(60 * 60_000), "1h 00m");
        assert_eq!(fmt_duration(95 * 60_000), "1h 35m");
    }

    #[test]
    fn fmt_accuracy_returns_dash_for_zero_submits() {
        let t = Totals::default();
        assert_eq!(fmt_accuracy(&t), "--");
    }

    #[test]
    fn fmt_accuracy_rounds_to_nearest_percent() {
        let t = Totals { active_ms: 0, submits: 7, correct: 6, words: 0 };
        assert_eq!(fmt_accuracy(&t), "86%"); // 6/7 = 0.857… → 86%
    }

    #[test]
    fn bar_for_full_when_value_equals_max() {
        assert_eq!(bar_for(10, 10, 10), "██████████");
    }

    #[test]
    fn bar_for_empty_when_max_is_zero() {
        assert_eq!(bar_for(0, 0, 5), "░░░░░");
    }

    #[test]
    fn renders_empty_state_without_panic() {
        let backend = TestBackend::new(80, 20);
        let mut term = Terminal::new(backend).unwrap();
        let stats = Stats::empty();
        let today = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
        term.draw(|f| render_stats(f, f.area(), &stats, today, StatsView::Weekly))
            .unwrap();
    }

    #[test]
    fn renders_full_state_without_panic() {
        let backend = TestBackend::new(80, 30);
        let mut term = Terminal::new(backend).unwrap();
        let mut stats = Stats::empty();
        stats.days.insert(
            NaiveDate::from_ymd_opt(2026, 5, 13).unwrap(),
            t(743_000, 24, 22, 168),
        );
        stats.days.insert(
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            t(180_000, 31, 27, 215),
        );
        let today = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
        term.draw(|f| render_stats(f, f.area(), &stats, today, StatsView::Weekly))
            .unwrap();
        term.draw(|f| render_stats(f, f.area(), &stats, today, StatsView::Monthly))
            .unwrap();
    }
}
```

Note: `ratatui::backend::TestBackend` is in the default feature set; no Cargo change needed.

- [ ] **Step 2: Run tests**

Run: `cargo test --lib ui::stats::tests`
Expected: 7 tests pass.

Full suite:

Run: `cargo test`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/ui/stats.rs
git commit -m "feat(stats): full /stats view with weekly + monthly tables"
```

---

### Task 9: Study mini-strip (today summary)

**Files:**
- Modify: `src/app.rs::render_bottom_banners` (add new row at the top of the bottom stack)

- [ ] **Step 1: Locate `render_bottom_banners`**

It's at `src/app.rs:~1206`. Banners stack from `last_row` upward using `row_from_bottom`. The mini-strip is the **lowest-priority** entry, so when no other banner exists it lives on the bottom row; when banners are present it sits one row above them.

- [ ] **Step 2: Add the mini-strip row**

After the existing TTS-disabled block, append:

```rust
        if matches!(self.screen, Screen::Study) {
            let t = self.stats.today_stats();
            let acc = if t.submits == 0 {
                "--".to_string()
            } else {
                let pct = (t.correct as f64 * 100.0 / t.submits as f64).round() as u32;
                format!("{}%", pct)
            };
            let text = format!(
                "{} · {}w · {}",
                crate::ui::stats::fmt_duration(t.active_ms),
                t.words,
                acc
            );
            let y = last_row.saturating_sub(row_from_bottom);
            let para = Paragraph::new(Line::from(text))
                .style(Style::default().fg(Color::DarkGray))
                .right_aligned();
            frame.render_widget(para, Rect::new(inner.x, y, inner.width, 1));
            // intentionally do NOT bump row_from_bottom here — the mini-strip
            // is the bottom-most info and shouldn't push other banners up
            // beyond what they already need.
        }
```

Make `fmt_duration` reachable from outside the `ui::stats` module — it's already `pub(crate)` in Task 8. Verify the import block in `app.rs` doesn't need a new line (the call uses the absolute path, which avoids reshuffling imports).

If the existing `render_bottom_banners` does not already have `use ratatui::widgets::Paragraph` and `use ratatui::text::Line` in scope at that callsite, add them at the top of `app.rs`:

```rust
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::layout::Rect;
```

(Search before adding to avoid duplicates.)

- [ ] **Step 3: Write a unit-style test for `today_stats` format**

In `src/ui/stats.rs`, add:

```rust
#[test]
fn mini_strip_format_no_data() {
    let t = Totals::default();
    assert_eq!(fmt_duration(t.active_ms), "0m");
    assert_eq!(fmt_accuracy(&t), "--");
}

#[test]
fn mini_strip_format_with_data() {
    let t = Totals { active_ms: 12 * 60_000 + 23_000, submits: 24, correct: 22, words: 168 };
    assert_eq!(fmt_duration(t.active_ms), "12m");
    assert_eq!(fmt_accuracy(&t), "92%");
}
```

These exercise the format helpers the mini-strip relies on, without trying to drive the full `App::render` loop.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/ui/stats.rs
git commit -m "feat(stats): today mini-strip at bottom of Study screen"
```

---

### Task 10: Hygiene — fmt, clippy, full test pass

**Files:** (no source changes expected unless lint fixups are needed)

- [ ] **Step 1: Format the workspace**

Run: `cargo fmt --all`
Expected: SUCCESS, possibly no diff.

- [ ] **Step 2: Run clippy with -D warnings**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: SUCCESS.

If lints fire, fix in place — common culprits with new code:
- unused imports → remove them
- `&Vec<_>` → `&[_]`
- `format!` in a single-arg constructor → use `.into()` if cheaper
- `let _ = (today, view);` placeholders left in Task 7 — remove now if still present.

- [ ] **Step 3: Run the full test suite**

Run: `cargo test`
Expected: PASS. Total new tests: ~38–40.

- [ ] **Step 4: Manual smoke (record outcome in the commit message if anything notable)**

Run: `cargo run -- --help` (or simply `cargo run`), then:
1. Type a few characters in the Study screen → confirm the mini-strip in the bottom row updates after each Enter.
2. `Ctrl+P` → `/stats` → confirm view loads (empty state if you haven't typed anything today; otherwise three cards + week strip).
3. In `/stats`, press `m` → monthly view; `w` → weekly view; `Esc` → return to Study.
4. Quit with `Ctrl+P /quit` (or `Ctrl+C`) and verify `~/.config/inkworm/stats.json` exists and parses (`jq . ~/.config/inkworm/stats.json`).

If anything is off, file a follow-up — do not commit the failure.

- [ ] **Step 5: Commit any hygiene fixes**

Only if Step 1 or 2 produced edits:

```bash
git add <files-changed-by-fmt-or-clippy>
git commit -m "chore(stats): fmt + clippy fixups"
```

---

## Self-Review (post-write)

**Spec coverage** — each spec section mapped to a task:

| Spec §                | Task |
|---|---|
| §1 Goal               | full plan |
| §2 Decisions          | Tasks 2 (idle threshold), 5 (words), 6 (merge modes), 1 (per-day forever) |
| §3 Architecture       | Tasks 1–9 cover all listed files |
| §4 Stats data model   | Task 1 |
| §5 StatsTracker       | Task 2 |
| §6 Aggregation        | Task 4 |
| §7 App integration    | Tasks 5, 6 |
| §8 Full-screen UI     | Tasks 7, 8 |
| §9 Study mini-strip   | Task 9 |
| §10 Error handling    | Task 6 (save error → banner), Task 1 (load missing → empty) |
| §11 Known limitations | Tracker tests cover midnight straddle, clock rewind |
| §12 Out of scope      | None of these are implemented — confirmed |
| §13 Test plan         | All 5 test layers present |
| §14 Implementation order | Plan order matches |

**Placeholder scan:** No `TBD`, no "add error handling" without showing how, no `Similar to Task N` without repeating the code, no undefined types — every type used appears in the task that introduces it.

**Type consistency:**
- `StatsTracker` API: `from_stats`, `snapshot`, `today_stats`, `on_keystroke`, `on_submit`, `flush_idle` — same in Tasks 2, 4, 5, 6, 9.
- `SubmitTick { was_correct, words }` — same in Tasks 5, 6.
- `StatsView { Weekly, Monthly }` — same in Tasks 7, 8.
- `Totals` / `WeekDayCell` / `WeekRow` / `MonthRow` — defined in Task 4, used in Task 8.
- `iso_week_label` — defined in Task 4, used in Task 8.
- `fmt_duration` / `fmt_accuracy` — defined in Task 8 as `pub(crate)`, used in Task 9.

No drift detected.
