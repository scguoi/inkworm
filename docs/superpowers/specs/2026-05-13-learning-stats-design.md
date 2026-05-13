# Learning Statistics — Design

**Status:** Approved
**Date:** 2026-05-13
**Related:** `2026-04-21-inkworm-design.md` (root spec), `2026-04-27-inkworm-mistakes-design.md`

## 1. Goal

Show the user how much they have actually been studying — not how long the
process has been running. Three concrete numbers, viewable for today, this
week, and historically by week / month:

1. **Active study time** — wall-clock time during which the user is actually
   typing in the Study screen, with idle gaps over 30 seconds excluded.
2. **Words submitted** — total English words across every submitted drill
   sentence (each `submit()` contributes `count_words(drill.english)`,
   regardless of correctness or first-attempt status).
3. **Accuracy** — `correct_submits / total_submits`. Every call to
   `StudyState::submit` is counted; retries until correct contribute one
   correct on the successful attempt and one wrong per failed attempt.

Two surfaces:

- A full-screen `Screen::Stats` view reached via palette `/stats`.
- A one-line "today" mini-strip at the bottom of the Study screen.

Course mode and Mistakes mode contribute to the same totals.

Out of scope: WPM / keystroke-level metrics, Course-vs-Mistakes split views,
historical backfill from `progress.json`, monthly heatmaps, CSV export,
multi-device sync, per-course breakdown.

## 2. Decisions (frozen by brainstorming dialogue)

| # | Question | Decision |
|---|---|---|
| 1 | What counts as "study time"? | Keystroke-idle timeout. Time accumulates only when a keystroke arrives within 30s of the previous one. |
| 2 | What "words" are counted? | English words in each submitted drill sentence (`split_whitespace().count()`). |
| 3 | Accuracy formula | `correct_submits / total_submits` — every submit counted, retries included. |
| 4 | Course vs Mistakes | Merged into one total. |
| 5 | Retention | Per-day aggregates, kept forever (~3 KB/year). |
| 6 | UI surfaces | Full-screen `/stats` view **and** Study-screen mini-strip. |
| 7 | Configurable idle threshold? | No. `IDLE_THRESHOLD_MS = 30_000` is a `const`. |

## 3. Architecture

Three units, each independently testable, separated by the existing
`storage/ behavior / ui/` convention:

```
src/stats/
  ├── mod.rs        // pub use { Stats, DayStats, StatsTracker, count_words, ... }
  ├── tracker.rs    // StatsTracker — pure in-memory accounting, no IO
  └── aggregate.rs  // pure functions: aggregate_week, recent_weeks, recent_months, totals
src/storage/stats.rs // Stats serde + load/save (mirrors progress.rs structure)
src/ui/stats.rs      // StatsState + render_stats (full-screen view)
```

Changed files: `src/app.rs`, `src/ui/study.rs`, `src/ui/palette.rs`,
`src/storage/paths.rs`, `src/lib.rs`.

### 3.1 Data flow

```
keystroke in Study screen
   │
   ▼
App::handle_study_key
   │
   ├──> StatsTracker::on_keystroke(now)   // accumulates active_ms
   │
   └──> StudyState::on_char / submit / backspace ...

StudyState::submit(...)
   │
   ▼
(Option<SubmitOutcome>, Option<SubmitTick>)
   │
   ▼
App::handle_submit_outcome
   │
   ├──> StatsTracker::on_submit(now, was_correct, words)
   │
   ├──> stats.snapshot().save(path)   // once per submit; no batching
   │
   └──> mistakes book bookkeeping (unchanged)

tick (~60 Hz)
   │
   └──> StatsTracker::flush_idle(now)   // closes session if last keystroke > 30s ago

App::quit
   │
   └──> stats.save(path)        // final flush
```

## 4. Data model — `src/storage/stats.rs`

```rust
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
    pub fn empty() -> Self { /* schema_version = 1 */ }
    pub fn load(path: &Path) -> Result<Self, StorageError>;
    pub fn save(&self, path: &Path) -> Result<(), StorageError>;
}
```

**File**: `~/.config/inkworm/stats.json` (or `INKWORM_HOME/stats.json`).
`DataPaths::stats_file` added to `src/storage/paths.rs`; `ensure_dirs` does
not need to touch it (the file itself is created on first save by
`write_atomic`).

**Date keys** use `chrono::NaiveDate` serialized as `YYYY-MM-DD`. Date is
local-zone (`Local::now().date_naive()` style), matching the mistakes book's
`NaiveDate` convention.

**Load**: missing file → `Self::empty()` (matches `Progress::load`).
JSON-parse error → propagate `StorageError`; App startup surfaces via the
existing error banner.

**Save**: `write_atomic` (existing helper, fsync + rename + dir-fsync).

**Schema evolution**: new optional fields use `#[serde(default)]`. No
migration in v1.

**Unit tests in `src/storage/stats.rs`**:
- `empty_round_trips`
- `serde_uses_camel_case_keys`
- `load_missing_returns_empty`
- `day_stats_default_zero`

## 5. Tracker — `src/stats/tracker.rs`

```rust
pub const IDLE_THRESHOLD_MS: i64 = 30_000;

pub struct StatsTracker {
    last_activity: Option<DateTime<Utc>>,
    today: NaiveDate,                       // last-known local date
    today_stats: DayStats,                  // in-memory accumulator for `today`
    history: BTreeMap<NaiveDate, DayStats>, // everything else
}

impl StatsTracker {
    pub fn from_stats(stats: Stats, today: NaiveDate) -> Self;
    pub fn snapshot(&self) -> Stats;        // merges today_stats into history
    pub fn on_keystroke(&mut self, now: DateTime<Utc>);
    pub fn on_submit(&mut self, now: DateTime<Utc>, was_correct: bool, words: u32);
    pub fn flush_idle(&mut self, now: DateTime<Utc>);
    pub fn today_stats(&self) -> &DayStats; // for Study mini-strip
}
```

### 5.1 `on_keystroke(now)` algorithm

1. Compute `today_local = now.with_timezone(&Local).date_naive()`.
2. If `today_local != self.today`: move `self.today_stats` into
   `self.history`, set `self.today = today_local`, reset
   `self.today_stats = DayStats::default()`, and clear `last_activity`
   (a new day starts a new session — no straddle).
3. If `self.last_activity` is `Some(prev)` and `prev <= now` and
   `(now - prev) <= IDLE_THRESHOLD_MS`: add `(now - prev)` to
   `today_stats.active_ms`.
4. Otherwise (no prior activity, idle gap too long, or clock went
   backwards): do not accumulate; treat as new session.
5. Set `self.last_activity = Some(now)`.

**Invariant:** active time is accumulated only at the moment a keystroke
arrives, in chunks ≤ 30s. No background timer is required to advance time.

### 5.2 `on_submit(now, was_correct, words)`

1. Call `self.on_keystroke(now)` first — submits double as keystrokes for
   timing purposes.
2. `self.today_stats.submits += 1`
3. `if was_correct { self.today_stats.correct += 1 }`
4. `self.today_stats.words += words`

### 5.3 `flush_idle(now)` (called from `App::on_tick`)

If `last_activity` is `Some(prev)` and `now - prev > IDLE_THRESHOLD_MS`,
set `last_activity = None`. **No time is added.** This only guarantees that
the next keystroke after a long pause is treated as a session start.

### 5.4 Clock-anomaly handling

- `now < last_activity` (clock rewind, NTP jump): treat as session start,
  delta = 0. Time already accumulated for the day is **not** rolled back.
- Submits during clock rewind still increment counters; only timing is
  conservative.

### 5.5 Unit tests in `src/stats/tracker.rs`

- `first_keystroke_records_zero_time`
- `keystrokes_within_threshold_accumulate_delta`
- `gap_over_threshold_does_not_accumulate`
- `submit_increments_correct_and_words`
- `submit_increments_total_even_when_wrong`
- `day_rollover_moves_today_to_history_and_resets`
- `day_rollover_clears_last_activity` (no carry-over delta across midnight)
- `flush_idle_clears_session_after_30s`
- `flush_idle_noop_when_within_threshold`
- `clock_rewind_does_not_subtract_time`
- `snapshot_round_trips_through_stats`

## 6. Aggregation — `src/stats/aggregate.rs`

Pure functions, no state, no IO. Inputs are `&Stats` plus a "today" anchor.

```rust
pub struct Totals { pub active_ms: u64, pub submits: u32, pub correct: u32, pub words: u32 }
pub struct WeekDayCell { pub date: NaiveDate, pub stats: DayStats, pub is_today: bool }
pub struct WeekRow    { pub iso_year: i32, pub iso_week: u32, pub totals: Totals }
pub struct MonthRow   { pub year: i32, pub month: u32, pub totals: Totals }

pub fn today_totals(stats: &Stats, today: NaiveDate) -> Totals;
pub fn week_totals(stats: &Stats, today: NaiveDate) -> Totals;     // Mon..Sun containing today
pub fn all_time_totals(stats: &Stats) -> Totals;
pub fn this_week(stats: &Stats, today: NaiveDate) -> [WeekDayCell; 7]; // Mon..Sun
pub fn recent_weeks(stats: &Stats, today: NaiveDate, n: usize) -> Vec<WeekRow>;   // newest first; excludes today's ISO week (which is the "This week" strip)
pub fn recent_months(stats: &Stats, today: NaiveDate, n: usize) -> Vec<MonthRow>; // newest first; includes today's month
```

Week boundary is **ISO 8601 (Mon–Sun)**. Use `chrono::Datelike::iso_week()`
to bucket rows by `(iso_year, iso_week)`.

Accuracy is computed at render-time from `Totals`: `if submits == 0 { "--" }
else { round(100 * correct / submits) }`. Never persist accuracy.

### Unit tests in `src/stats/aggregate.rs`

- `today_totals_picks_correct_day`
- `today_totals_returns_zero_for_empty_stats`
- `week_totals_sums_mon_through_sun`
- `this_week_marks_today_cell`
- `this_week_handles_year_boundary` (today = Jan 1)
- `recent_weeks_orders_newest_first_and_pads_empty`
- `recent_weeks_excludes_current_iso_week`
- `recent_months_includes_current_month`
- `recent_months_handles_year_rollover`
- `all_time_totals_sums_every_day`

## 7. App integration — `src/app.rs`, `src/ui/study.rs`

### 7.1 New field

```rust
pub struct App {
    // ... existing ...
    stats: StatsTracker,
}
```

Constructed in `App::new` (or wherever `Progress::load` is called today):
```rust
let stats = Stats::load(&data_paths.stats_file)?;
let today = clock.today_local();
let stats = StatsTracker::from_stats(stats, today);
```

### 7.2 `SubmitTick` signal

The current `StudyState::submit` returns `Option<SubmitOutcome>`, produced
only on the **first** attempt for a given drill (`first_attempt_pending`
gate). The "total submits" accuracy formula requires a per-call signal,
including retries. Change the signature to:

```rust
pub struct SubmitTick { pub was_correct: bool, pub words: u32 }

pub fn submit(&mut self, clock: &dyn Clock) -> (Option<SubmitOutcome>, Option<SubmitTick>);
```

`SubmitTick` is produced on every successful `submit()` call (i.e. when the
phase is `Active` and feedback is `Typing`). `words` is `drill.english
.split_whitespace().count() as u32`. `SubmitOutcome` semantics are
unchanged — mistakes-book wiring is untouched.

### 7.3 Keystroke hook

In `App::handle_study_key`, before dispatching the character / backspace /
submit, add:
```rust
self.stats.on_keystroke(self.clock.now());
```
**Only** in `handle_study_key`. Keys typed in palette, course list,
wizard, etc. do not count as "studying".

### 7.4 Submit hook

In `App::handle_submit_outcome` (or wherever the `(outcome, tick)` tuple is
unpacked), call:
```rust
if let Some(tick) = tick {
    self.stats.on_submit(self.clock.now(), tick.was_correct, tick.words);
    if let Err(e) = self.stats.snapshot().save(&self.data_paths.stats_file) {
        tracing::warn!("Failed to save stats: {e}");
        self.info_banner = Some(format!("Failed to save stats: {e}"));
    }
}
```
Pattern mirrors `save_mistakes` (`src/app.rs:262`). Saves are one-per-submit,
not batched; submits are rare enough (a few per minute) that this is cheaper
than introducing a dirty-flag or timer.

### 7.5 Tick hook

In `App::on_tick`, add:
```rust
self.stats.flush_idle(self.clock.now());
```

### 7.6 Quit hook

In `App::quit` (or equivalent shutdown path), call
`self.stats.snapshot().save(...)` once more, ignoring errors.

### 7.7 `count_words` helper

```rust
// src/stats/mod.rs
pub fn count_words(english: &str) -> u32 {
    english.split_whitespace().count() as u32
}
```

### 7.8 Integration tests in `tests/stats.rs`

- `submit_correct_increments_today_correct_and_words`
- `submit_wrong_increments_submits_but_not_correct`
- `retry_until_correct_counts_each_submit` (key test for "total submits"
  accuracy semantics)
- `mistakes_mode_submit_contributes_to_same_day`
- `keystrokes_outside_study_screen_do_not_accumulate_time`
- `stats_file_written_after_each_submit_atomically`
- `app_quit_flushes_pending_stats`

## 8. UI — full-screen `Screen::Stats`

### 8.1 Entry / exit

- Palette command `/stats` (`src/ui/palette.rs::COMMANDS`) →
  `App::open_stats()` sets `self.screen = Screen::Stats`.
- `Esc` returns to the previous screen (Study or wherever palette was
  invoked from). Reuse the existing palette-return pattern.
- View hotkeys: `m` switches to "Last 12 months"; `w` switches back to
  "This week + Last 12 weeks" (the default). Tracked in
  `StatsState::view: StatsView { Weekly, Monthly }`.

### 8.2 Layout (Weekly view, default)

```
┌─ Stats ───────────────────────────────────────────────────────┐
│  Today      12m 23s    24 submits    22 correct (92%)   168w  │
│  This week  1h 48m    142 submits   126 correct (89%)  1024w  │
│  All time   34h 12m  4521 submits  4001 correct (88%) 32104w  │
├───────────────────────────────────────────────────────────────┤
│  This week (Mon–Sun, ISO 2026-W20)                            │
│    Mon 05-11  ████████░░  18m   31/35  88%   215w             │
│    Tue 05-12  ██████████  22m   28/31  90%   198w             │
│    Wed 05-13  ████████░░  18m   24/26  92%   168w  ← today    │
│    Thu 05-14  ░░░░░░░░░░  --                                  │
│    Fri 05-15  ░░░░░░░░░░  --                                  │
│    Sat 05-16  ░░░░░░░░░░  --                                  │
│    Sun 05-17  ░░░░░░░░░░  --                                  │
├───────────────────────────────────────────────────────────────┤
│  Previous 12 weeks (excludes current week)                    │
│   ISO Week   Time   Submits   Acc    Words                    │
│   2026-W19   3h 02   412      87%    3120                     │
│   2026-W18   2h 41   389      89%    2980                     │
│   ...                                                         │
└───────────────────────────────────────────────────────────────┘
   esc back   m: monthly view
```

### 8.3 Layout (Monthly view)

```
┌─ Stats ───────────────────────────────────────────────────────┐
│  [Today / This week / All time cards — same as weekly]        │
├───────────────────────────────────────────────────────────────┤
│  Last 12 months (includes current month)                      │
│   Month     Time     Submits   Acc    Words                   │
│   2026-05   14h 22   1842      88%    14210                   │
│   2026-04   11h 03   1521      87%    11420                   │
│   ...                                                         │
└───────────────────────────────────────────────────────────────┘
   esc back   w: weekly view
```

### 8.4 Rendering details

- Bar width is normalized to the **max `active_ms` in the displayed
  week**, drawn over a 10-column track using full / shaded / empty block
  chars (`█`, `░`). A zero-data day shows an empty track and `--`.
- Time formatter: `< 60s → "0m"`, `< 60min → "Xm"`, else `"Xh YYm"`.
- Accuracy formatter: `submits == 0 → "--"`, else `"{round(100*c/s)}%"`.
- Empty state (no days at all): center text "No data yet — start
  studying with /go" in place of the week strip and table.
- Layout uses `ratatui::Layout` with three vertical chunks: cards (3
  lines), week strip (9 lines: header + 7 + spacer), and recent table
  (remaining). On narrow terminals (< 60 cols) the cards collapse to
  one number per line. Bars collapse first when width is constrained.

### 8.5 State & re-render

`StatsState { view: StatsView, today: NaiveDate, snapshot: Stats }`. The
snapshot is taken once on enter (`app.stats.snapshot()`) and reused for the
duration of the view — this keeps render deterministic and avoids
re-aggregating on every frame. The user can leave and re-enter `/stats` to
refresh; no `r` keybinding in v1. The mini-strip on Study reads from
`app.stats.today_stats()` every render; no caching.

### 8.6 Snapshot tests in `tests/stats_ui.rs` (insta)

- `empty_state_render`
- `weekly_view_partial_week`
- `weekly_view_full_week_with_today_marker`
- `monthly_view_recent_12_months`
- `narrow_terminal_collapses_layout` (40-col)

### 8.7 Palette test in `tests/stats_palette.rs`

- `slash_stats_opens_stats_screen`
- `esc_returns_to_previous_screen`

## 9. Study mini-strip

Render one line at the bottom of `render_study`, right-aligned in the
existing status bar area:

```
12m · 24w · 92%
```

- Format: `{active_time} · {words}w · {acc}%`
- Empty-day display: `0m · 0w · --`
- Source: `app.stats.today_stats()` — directly, no aggregation.
- No new widget. Reuses the same status-bar Block/Paragraph that today
  carries the nav hints.

Snapshot test added alongside existing `tests/ui.rs` study renders.

## 10. Error handling

| Failure | Behavior |
|---|---|
| `Stats::load` file missing | `Self::empty()` |
| `Stats::load` JSON parse error | Propagate `StorageError`; App startup banner |
| `Stats::save` failure (mid-session) | `tracing::warn!` + `info_banner`, do not block input. Next submit retries. |
| `Stats::save` failure (on quit) | Log, ignore. Worst case: lose the session's pending counters since last successful save. |
| `count_words("")` | Returns 0 |
| Tracker time rewind | Treat as new session, delta = 0, no negative time |
| Aggregate over empty Stats | Returns zeroed totals / empty Vec; UI shows empty state |

## 11. Known limitations

1. **Midnight straddle**: A keystroke pair across midnight is bucketed
   into the later day (rollover happens at the entry point, before delta
   is computed). Acceptable.
2. **Clock rewind**: Already-accumulated time is not rolled back. The
   only safe alternative is "discard the day", which we judge worse.
3. **Crash mid-session**: All counters since the last successful `save`
   are lost. v1 does not introduce a `stats_session.json` intermediate
   (rejected in brainstorming for YAGNI; existing `instance_lock.rs`
   already prevents concurrent inkworm instances). Maximum loss is one
   submit-burst between two consecutive submits — minimal.
4. **Per-course / per-mode breakdown**: not stored; needs schema
   evolution.
5. **Backfill**: brand-new users start from zero; existing `progress.json`
   is **not** mined retroactively (no time-window data there).

## 12. Out of scope (will not be built in v1)

- WPM / keystroke-level counters
- Course-vs-Mistakes split views
- Monthly calendar heatmap (the 12-month table is the substitute)
- Third-party chart crates
- CSV / JSON export
- Cross-device sync
- Per-course statistics
- A configurable idle threshold

## 13. Test plan summary

| Layer | File | Count (approx) |
|---|---|---|
| Tracker unit | `src/stats/tracker.rs` | 11 |
| Aggregate unit | `src/stats/aggregate.rs` | 10 |
| Storage unit | `src/storage/stats.rs` | 4 |
| App integration | `tests/stats.rs` | 7 |
| UI snapshot | `tests/stats_ui.rs` | 5 |
| Palette | `tests/stats_palette.rs` | 2 |
| **Total new tests** | | **~39** |

Existing baseline (per memory: ~384) → expected ~420 after this work.

## 14. Implementation order (for the plan)

Each step ships independently; CI green after every step.

1. `src/storage/stats.rs` + `paths.rs::stats_file` + storage unit tests
2. `src/stats/tracker.rs` + tracker unit tests
3. `src/stats/aggregate.rs` + aggregate unit tests
4. `StudyState::submit` returns `(Option<SubmitOutcome>, Option<SubmitTick>)`
   + adjust existing callers (mistakes wiring is unchanged; only the
   call sites unpack the new tuple)
5. App integration: load stats, keystroke / submit / tick / quit hooks,
   integration tests
6. Palette `/stats` + `Screen::Stats` skeleton (entry/exit, empty state)
7. Full stats view rendering (weekly + monthly) + snapshot tests
8. Study mini-strip + study snapshot update
