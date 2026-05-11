# iCloud-aware Bundle Prewarm — Design

**Status:** Approved
**Date:** 2026-05-11
**Related:** `2026-05-07-bundled-course-audio-design.md` (bundle layout and lookup)

## 1. Goal

When inkworm starts or the user switches courses, **download every bundled
mp3 of the active course from iCloud to local storage in the background**,
fast enough and visibly enough that playback during study never blocks on a
per-drill iCloud fetch. Strictly distinguish "the file is locally resident"
from "the file appears to exist but is an iCloud `dataless` placeholder",
so the bundle gate cannot lure `rodio` into a blocking `File::open`.

Out of scope: prewarming inactive courses, prewarming on user hover in the
course list, persisting a "warmed files" manifest, retrying transient
iCloud failures.

## 2. Background — what's broken today

Three latent defects in v0.2.9 combine into "this course plays no audio":

1. **`bundle_exists` cannot see placeholders.** It uses `path.is_file()`,
   which returns `true` for an iCloud dataless file even when the file has
   zero physical blocks. The bundle gate in `App::speak_current_drill`
   therefore routes to `BundlePlayer::play`, which calls `std::fs::File::open`,
   which blocks on iCloud's synchronous download.
2. **`spawn_prewarm_course` is sequential.** It walks every drill mp3 in a
   single async `for` loop, awaiting one `spawn_blocking(materialize)` at a
   time. For a 40-drill course over Bluetooth tethering this takes a minute
   or more; the user reaches drill 4 well before file 4 is downloaded.
3. **Per-file prewarm failures and progress are invisible.** Errors log at
   `debug` level (effectively hidden), success logs at `debug` too, and the
   UI shows nothing. The user has no way to tell prewarm is even running.

The bug surfaces as: course directory looks complete in Finder (40 mp3s,
sizes correct), but `stat -f %b` shows `blocks=0` on most of them; rodio's
`symphonia` demuxer logs once or twice then silence.

## 3. Architecture

Three independent units, each with a single responsibility:

### 3.1 `audio::bundle::is_locally_resident(path) -> bool` and `is_icloud_placeholder(path)`

Two predicates, both pure:

- `is_locally_resident`: On Unix, `path.metadata().blocks() > 0`. Returns
  `false` for missing paths (metadata error) and for zero-block files
  (iCloud `dataless` placeholders). On non-Unix, returns `true` whenever
  metadata reads.
- `is_icloud_placeholder = path.is_file() && !is_locally_resident(path)`.
  Returns `true` only for *files that exist but have zero physical blocks*
  — the on-disk signature of an iCloud `dataless` file. Returns `false`
  for missing paths (which mean "this course shipped without bundle
  audio", not "needs download"), for directories, and for files with
  blocks > 0.

The distinction matters: the bundle gate (`bundle_exists`, the inline
gate in `speak_current_drill`) cares about "can I safely open this without
blocking on iCloud?" — that's `is_locally_resident`. The prewarm filter
cares about "should I try to download this?" — that's `is_icloud_placeholder`.
Conflating them turned every course-without-bundle-audio into a
"40 files unavailable" banner; the prewarm spun up 40 jobs that all
failed with `NotFound`.

`bundle_exists` is rewritten as `path.is_file() && is_locally_resident(path)`.

### 3.2 `audio::bundle::spawn_prewarm_course` — concurrent + cancellable

Signature changes from `(courses_dir: PathBuf, course: &Course)` to:

```rust
pub fn spawn_prewarm_course(
    courses_dir: PathBuf,
    course: &Course,
    generation: u64,
    current_generation: Arc<AtomicU64>,
    progress_tx: tokio::sync::mpsc::Sender<TaskMsg>,
)
```

Internally:

- Resolve full path list via existing `bundle_paths_for_course`.
- Keep only paths where `is_icloud_placeholder(p)` returns `true` —
  drops both already-resident files (cheap re-warm) and entirely
  missing files (course has no bundle, prewarm would just fail).
- Use `futures::stream::iter(remaining_paths).for_each_concurrent(8, ...)`.
- Each unit of work is `tokio::task::spawn_blocking(move || materialize(&p))`
  followed by an atomic `done` counter and a `TaskMsg::PrewarmProgress`
  send.
- Before kicking off each `spawn_blocking`, the task checks
  `current_generation.load(Acquire) == generation`; if not, the stream is
  short-circuited with `take_while` and no further files are touched.
- When the stream completes (or short-circuits), send a single
  `TaskMsg::PrewarmDone { generation, ok, failed }`.
- Errors per file: `tracing::warn!` with the path; counted into `failed`;
  never propagated up.

The concurrency cap is **8**. iCloud Drive's CloudKit backend serializes
heavily past 4–8 parallel fetches; raising it further does not measurably
speed up downloads and just burns blocking-pool threads.

### 3.3 `App` prewarm orchestration

`App` gains two fields:

```rust
prewarm_generation: Arc<AtomicU64>,   // monotonic, incremented per launch
prewarm_state: Option<PrewarmState>,  // banner-relevant snapshot
```

where `PrewarmState { generation: u64, done: u32, total: u32 }`. The
`Arc<AtomicU64>` is cloned into every spawned prewarm task; the
`prewarm_state` lives only on the main thread.

`spawn_bundle_prewarm` becomes:

1. `generation = self.prewarm_generation.fetch_add(1, Release) + 1`.
2. Compute `total = bundle_paths_for_course(...).iter().filter(|p| !is_locally_resident(p)).count()`.
3. If `total == 0`: skip — no banner, no spawn, no message. (Happy path
   after the first session per course.)
4. Otherwise set `self.prewarm_state = Some(PrewarmState { generation, done: 0, total })`,
   set `self.info_banner = Some("Prewarming audio (0/N)…")`, then spawn
   the prewarm task.

The three existing call sites (`App::new`, `switch_to_course`,
`enter_mistakes_mode_at_current_drill`) keep their current call to
`spawn_bundle_prewarm` — no logic moves to them.

## 4. Data Flow

```
App::new / switch_to_course / enter_mistakes_mode_at_current_drill
  │
  ▼
spawn_bundle_prewarm()
  │   bumps prewarm_generation, sets banner, spawns task
  ▼
audio::bundle::spawn_prewarm_course (tokio task)
  │   stream::for_each_concurrent(8)
  │   ├─ check generation; if stale → take_while halts stream
  │   ├─ spawn_blocking(materialize(path))
  │   ├─ TaskMsg::PrewarmProgress { generation, done, total }
  │   └─ on stream end → TaskMsg::PrewarmDone { generation, ok, failed }
  ▼
App main loop (handle_task_msg)
  │   if msg.generation != prewarm_state.generation → drop
  ▼
  banner update / clear
```

## 5. TaskMsg additions

```rust
pub enum TaskMsg {
    // ... existing variants
    PrewarmProgress { generation: u64, done: u32, total: u32 },
    PrewarmDone { generation: u64, ok: u32, failed: u32 },
}
```

Handling in `App`:

- `PrewarmProgress`: if `generation != self.prewarm_state.as_ref().map(|s| s.generation)` then drop. Otherwise update `state.done`, refresh banner to `format!("Prewarming audio ({}/{})…", done, total)`.
- `PrewarmDone`: same generation guard. Then:
  - `failed == 0` → clear `prewarm_state` and `info_banner`.
  - `failed > 0` → clear `prewarm_state`; set banner to
    `format!("Audio ready ({}/{}), {} files unavailable", ok, ok + failed, failed)`.
    The banner is cleared on the next user keypress like every other
    transient info banner (existing behavior).

## 6. Cancellation semantics

When the user switches courses while a prewarm is mid-flight:

1. `App::switch_to_course` calls `spawn_bundle_prewarm`, which bumps
   `prewarm_generation` (e.g. 4 → 5).
2. The in-flight task for generation 4 sees its `take_while` predicate
   `current_generation.load() == 4` fail on the next iteration and stops
   spawning new `spawn_blocking` jobs. The 0–8 jobs already in flight are
   not cancelled — `spawn_blocking` is not abortable — but they only
   download files that may be needed later anyway, and their `Progress`
   messages are dropped by the generation guard in `App`.
3. The new task for generation 5 starts immediately; banner is updated
   to its `total`.

This is "best-effort cancellation" — bandwidth is bounded by the in-flight
window (≤ 8 files), which is acceptable.

## 6.5. Banner interaction with `handle_study_key`

`App::handle_study_key` already clears `info_banner` on any keypress as
its first action (consuming the key). That's fine for transient banners
like "Copied to clipboard" or a boot warning, but it must NOT eat the
user's first keypress just to dismiss the prewarm progress line. The
fix is to skip that early-return when `prewarm_state.is_some()` — i.e.
a prewarm is in flight. Once prewarm completes (success → banner is
already cleared; failure → banner shows "Audio ready ..., K unavailable"
with `prewarm_state = None`), keypress dismissal resumes its normal
behavior.

## 7. Behavior matrix

| Scenario | Bundle file state | `bundle_exists` | `speak_current_drill` route |
|---|---|---|---|
| Before this fix, fresh course | placeholder | `true` | bundle path → blocks in `File::open` |
| After this fix, prewarm in progress, file not yet warmed | placeholder | **`false`** | falls through to TTS (or silence if no creds / off) |
| After this fix, prewarm complete | resident | `true` | bundle path → plays |
| File missing entirely | missing | `false` | falls through to TTS (unchanged) |
| iCloud unavailable, prewarm failed | placeholder | `false` | TTS fallback until network returns |
| Non-Unix target | regular file | `true` (always) | bundle path → plays |

## 8. Testing

Unit tests added/changed:

- `is_locally_resident`:
  - Returns `false` for a freshly-created zero-byte file (`blocks=0`).
  - Returns `true` for a file with non-empty content (`blocks > 0`).
  - Returns `false` for a missing path.
- `bundle_exists` existing tests: update the two tests that create
  zero-byte placeholder mp3s (`bundle_exists_true_when_file_present`,
  `bundle_exists_false_for_other_stage`) to write a non-empty body so they
  exercise the new joint predicate.
- `spawn_prewarm_course`:
  - With `current_generation` matching: all files materialize, exactly
    `total + 1` messages arrive on the channel (N progress + 1 done).
  - With `current_generation` already incremented past the task's
    generation: the task exits after at most 8 in-flight files; final
    `PrewarmDone` still arrives with `ok + failed ≤ 8`.
  - With a mix of already-resident and placeholder files (simulated by
    writing real content vs. truncating to 0 bytes), `total` reflects only
    the placeholder count.
- `App` integration: a small test driving the message handler verifies
  - Progress with stale generation is dropped.
  - `PrewarmDone { failed: 0 }` clears banner.
  - `PrewarmDone { failed: 2, ok: 38 }` sets the failure banner string.

No integration test will exercise real iCloud — the existing approach
(unit-test against tempdir + manually-written files) is sufficient.

## 9. Migration / compatibility

- No on-disk format changes.
- No config schema changes.
- Existing courses already on disk: first launch after upgrade triggers
  one prewarm pass; subsequent launches see `total == 0` and silently no-op.
- No version bump beyond a normal patch release (post-v0.2.9).

## 10. Why not — rejected alternatives

- **Prewarm all courses on startup.** Punted (YAGNI). Today only the
  active course's bundle is consulted at speak time. Predownloading every
  course is up to dozens of MB on first run, hurts battery and tethered
  data plans, and provides no benefit until the user actually switches.
- **xattr-based placeholder detection.** Apple's iCloud xattr names have
  shifted across macOS versions (`com.apple.clouddocs.*` → `com.apple.cloud.*`);
  `blocks() == 0` is a stable, kernel-level signal that's also trivially
  correct on Linux/Windows where iCloud doesn't exist.
- **`BundlePlayer::play` decode timeout with TTS fallback.** Once
  `bundle_exists` strictly excludes placeholders, the play path cannot
  reach a `File::open` that blocks on iCloud. A decode-side timeout is
  defending against a problem that no longer occurs.
- **Status-bar X/N indicator.** The info banner already has a slot for
  exactly this kind of transient state; reusing it costs zero UI work.
- **Persisting a "warmed files" manifest.** `blocks() > 0` is the manifest
  — it's a real property of the file on disk, not a piece of state we
  have to keep in sync.
