# Bundled Course Audio — Design

**Status:** Approved
**Date:** 2026-05-07
**Related:** `2026-05-06-courses-yyyy-mm-subdir-design.md` (course directory layout)

## 1. Goal

Allow a course to ship pre-generated mp3 audio for each drill. When a drill
is studied and a matching mp3 exists alongside the course file, play it
directly — bypassing the iFlytek WS path and its wav cache entirely. When
audio is missing or corrupt, fall back to the existing TTS pipeline so
behavior degrades transparently.

External tools (operated outside this repo) generate both the course JSON
and its audio bundle and write them to disk; inkworm only consumes them.

## 2. On-disk Layout

Audio lives in a sibling directory of the course JSON, named after the
course id (without the `.json` extension):

```
<courses_dir>/2026-05/
├── 06-foo.json                        # course schema (existing)
└── 06-foo/                            # bundle dir (new)
    ├── s01-d1.mp3                     # sentence 1, drill stage 1
    ├── s01-d2.mp3
    ├── s01-d3.mp3
    ├── s02-d1.mp3
    └── ...
```

**Naming:** `s{order:02}-d{stage}.mp3`
- `order` is the 1-based sentence index (1..=20), padded to two digits
- `stage` is the 1-based drill stage (1..=5), single digit (no padding)

The naming rule is mechanical so an LLM can produce it from a course
without ambiguity.

**Granularity:** one mp3 per drill (not per sentence). Different stages
within the same sentence have different `english` text, so per-drill is
the only granularity that yields 100% cache hits.

**Path resolution** reuses the existing yyyy-mm split from `course_path`:
the bundle dir is at `<courses_dir>/<yyyy-mm>/<id-without-yyyy-mm>/`
where `<id-without-yyyy-mm>` is `id[8..]` (e.g. `06-foo` for
`2026-05-06-foo`).

## 3. Lookup Order

When `App::speak_current_drill` runs:

```
1. Resolve bundle path: <courses_dir>/<yyyy-mm>/<id-tail>/s{order:02}-d{stage}.mp3
   ├─ path.exists() → spawn decode + rodio playback (decode failure logged, silent on this drill — see §7)
   └─ path missing → fall through to step 2

2. speaker.speak(drill.english)                              (existing path, unchanged)
   ├─ <tts-cache>/<blake3(text+voice)>.wav hit → play wav
   └─ miss → iFlytek WS → write wav cache → play
```

Fall-through is decided synchronously at `path.exists()` time. Once we
commit to the bundle path and spawn the decode task, we do not re-route
to TTS even if decode fails (§7 explains why).

### Constraints

- Bundle hits **never** touch the global wav cache: no read, no write.
  Mp3 bytes ≠ wav PCM, transcoding has no value.
- `tts_session_disabled` and missing iFlytek credentials do **not** block
  bundle playback. The bundle path is purely local.
- The `should_speak()` device check (headphone/output detection) still
  applies to both paths — when the device is unsuitable, both paths skip.
- Cancellation: drill change cancels the bundle sink and the speaker
  together (`bundle_player.cancel()` + `speaker.cancel()`).

### Why fall-through is per-drill (not per-course)

A course bundle may be partial (the generation tool failed on some drill,
or a newly-added drill hasn't been regenerated yet). Treating each drill
independently lets a partial bundle still benefit users without an
all-or-nothing switch.

## 4. Module Layout

A new top-level `src/audio/` module (not under `tts/` or `storage/`):

```
src/audio/
├── mod.rs                 # re-exports
├── bundle.rs              # path resolution + existence check (pure)
└── player.rs              # mp3 decode + rodio Sink + cancel
```

Why a new module: bundle audio depends on `rodio` for playback but has
zero overlap with iFlytek auth, signing, frame parsing, or wav cache. Co-
locating with `tts/` would imply a coupling that doesn't exist; co-
locating with `storage/` (pure data) would conflate audio playback with
data access. A separate `audio/` module also leaves room for future audio
features (typing sound effects, etc.) without naming churn.

### `bundle.rs`

```rust
pub fn bundle_path(
    courses_dir: &Path,
    course_id: &str,
    order: u32,
    stage: u32,
) -> Result<PathBuf, StorageError>;

pub fn bundle_exists(
    courses_dir: &Path,
    course_id: &str,
    order: u32,
    stage: u32,
) -> bool;
```

`bundle_path` validates the course id via the existing
`has_yyyy_mm_dd_prefix` helper (re-exported from `storage::course`) and
returns `StorageError::InvalidId` on mismatch. `bundle_exists` is a
non-failing convenience wrapper (returns `false` on any error).

### `player.rs`

```rust
pub struct BundlePlayer {
    audio: Option<rodio::OutputStreamHandle>,
    current_sink: Arc<Mutex<Option<rodio::Sink>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("io: {0}")] Io(#[from] std::io::Error),
    #[error("decode: {0}")] Decode(String),
    #[error("audio: {0}")] Audio(String),
}

impl BundlePlayer {
    pub fn new(audio: Option<rodio::OutputStreamHandle>) -> Self;

    /// Decode `path` as mp3, queue into a fresh rodio Sink.
    /// Returns Ok(()) immediately on `audio=None` (cache-only / headless
    /// mode used by tests). Replaces any prior sink (mirrors IflytekSpeaker).
    pub async fn play(&self, path: &Path) -> Result<(), BundleError>;

    /// Stop any active sink. Safe to call when nothing is playing.
    pub fn cancel(&self);
}
```

Decode runs in a `tokio::task::spawn_blocking` (rodio's `Decoder::new`
does sync IO + parse). The decoded `Source` is appended to a fresh Sink;
the previous sink (if any) is dropped without explicit stop, matching the
existing IflytekSpeaker convention.

## 5. App Integration

### Wiring (`src/main.rs`)

When constructing `OutputStream` and the speaker, also build a
`BundlePlayer` sharing the same `OutputStreamHandle`:

```rust
let bundle_player = Arc::new(BundlePlayer::new(audio_handle.clone()));
let speaker = build_speaker(&cfg.tts.iflytek, tts_cache_dir, mode, audio_handle);
```

Both get cheap clones of the handle (`OutputStreamHandle: Clone`).

### `App` field

```rust
pub struct App {
    // ...existing fields...
    bundle_player: Arc<audio::BundlePlayer>,
}
```

### Helpers to add on `StudyState`

`StudyState` exposes `current_drill()` and `current_course()` but not the
current sentence's `order`. Add one helper:

```rust
// src/ui/study.rs
pub fn current_sentence(&self) -> Option<&Sentence> {
    self.course.as_ref()?.sentences.get(self.sentence_idx)
}
```

`current_sentence().map(|s| s.order)` is what `App` consumes.

### `speak_current_drill` change

Insert a bundle-resolution step before the existing `speaker.speak`
spawn:

```rust
pub fn speak_current_drill(&self) {
    self.speaker.cancel();
    self.bundle_player.cancel();          // new: also cancel bundle sink

    if self.tts_session_disabled { /* ...existing skip... */ }
    if matches!(self.study.phase(), StudyPhase::Complete) { return; }
    let Some(drill) = self.study.current_drill() else { return; };
    let Some(sentence) = self.study.current_sentence() else { return; };
    let course_id = match self.study.progress().active_course_id.as_ref() {
        Some(id) => id.clone(),
        None => return self.speak_via_tts(&drill.english),
    };

    if !should_speak(self.config.tts.r#override, self.current_device, self.tts_has_creds()) {
        // device check still applies even for bundle: don't blast audio
        // when output is unsuitable
        return;
    }

    if let Ok(path) = audio::bundle_path(
        &self.data_paths.courses_dir,
        &course_id,
        sentence.order,
        drill.stage,
    ) {
        if path.exists() {
            let player = Arc::clone(&self.bundle_player);
            tokio::spawn(async move {
                if let Err(e) = player.play(&path).await {
                    // Decode failure: log and accept silence. We don't
                    // re-route to TTS from inside the spawn (see §7).
                    tracing::warn!("bundle playback failed: {e}");
                }
            });
            return;
        }
    }

    self.speak_via_tts(&drill.english);
}
```

`speak_via_tts` extracts the existing `tokio::spawn(async move { speaker.speak(...) })`
block verbatim — pure refactor, no behavior change. The `should_speak`
device check stays in its existing position (before either path) so it
gates both bundle and TTS uniformly.

**Note on bundle-decode-failure fallback:** if decode fails *after* the
file existed, we don't re-attempt TTS from inside the spawned task —
that would cross the App / async boundary in awkward ways and the user
has likely already moved on. We log and accept the silence. To get TTS,
the user can navigate away and back, which re-enters `speak_current_drill`
and (since `bundle.path.exists()` is still true but decode still fails)
will silently fall through... actually that won't help either. **This is
an accepted limitation:** corrupt mp3 = silent on that drill until the
user fixes the bundle. See §7.

### Cancellation

`speak_current_drill` calls both `speaker.cancel()` and
`bundle_player.cancel()` at entry. Either is safe when nothing is
playing on its respective path.

## 6. Edge Cases

| # | Case | Behavior |
|---|------|----------|
| 1 | Bundle dir doesn't exist | Fall through to TTS (zero-cost: single `path.exists()` check) |
| 2 | Bundle dir exists, specific drill mp3 missing | Single drill falls through to TTS |
| 3 | Mp3 file present but decode fails (corrupt / non-mp3 / 0 bytes) | `tracing::warn`, accept silence on this drill (do **not** fall through inside the async task — see §7 limitation) |
| 4 | Course id without yyyy-mm-dd prefix | `bundle_path` returns `Err(InvalidId)`; schema already rejects such ids, so unreachable in practice |
| 5 | `drill.order > 20` or `drill.stage > 5` | Schema-validated upstream; unreachable; format with `{:02}` / `{}` directly |
| 6 | Course dir evicted by iCloud (macOS) | Existing v0.2.4 boot-time iCloud download applies; runtime `File::open` errors → case #3 |
| 7 | Audio output device unavailable | `should_speak()` short-circuits both paths uniformly |
| 8 | User changes drill mid-mp3 | `cancel()` stops bundle sink synchronously |
| 9 | Stray files in bundle dir (README, orphan mp3) | Ignored: we only resolve paths, never enumerate the directory |
| 10 | Same text already in global wav cache, bundle also has mp3 | Bundle wins; wav stays untouched |
| 11 | Large mp3 file (>1MB) | rodio streams from `Source`; no full-buffer load; no size cap |
| 12 | Mp3 sample rate / channels differ from wav cache | rodio handles arbitrary mp3 specs; no constraint |

### Explicit non-goals

- No startup scan of bundle directories (no `O(courses)` cost at boot)
- No content-fingerprint check ("does this mp3 actually correspond to
  drill.english"); trust the generation tool
- No "ignore bundle, force TTS" toggle — to bypass, delete the directory

## 7. Known Limitations

- **Decode failure mid-spawn doesn't re-route to TTS.** If `bundle_path`
  exists but decode fails, the user gets silence on that drill. Fixing
  this would require either (a) sync decode in the App thread (blocks UI)
  or (b) channel-based "decoded → ok / failed → re-spawn TTS" plumbing.
  Neither is justified given the assumption that bundles are
  pre-validated by the generation tool.
- **Voice mismatch is invisible.** The bundled mp3 is whatever voice the
  generation tool used; the TTS fallback uses the configured iFlytek
  voice. A session may mix voices if some drills are bundled and others
  fall through. Acceptable for v1.

## 8. Testing

### Unit tests in `src/audio/`

`bundle.rs`:
- `bundle_path_yyyy_mm_split`: `("2026-05-06-foo", 1, 1)` → `<dir>/2026-05/06-foo/s01-d1.mp3`
- `bundle_path_pads_order_to_two_digits`: `(_, 9, 1)` → `s09-d1`; `(_, 12, 3)` → `s12-d3`
- `bundle_path_invalid_id_errors`: id without yyyy-mm-dd prefix → `InvalidId`
- `bundle_exists_false_when_dir_missing`
- `bundle_exists_true_when_file_present`: tempdir + empty `s01-d1.mp3`
- `bundle_exists_false_for_other_stage`

`player.rs` (with `audio=None` for headless):
- `play_with_no_audio_handle_is_noop`
- `play_decodes_real_mp3_fixture` (uses `fixtures/audio/silence.mp3`)
- `play_corrupt_file_returns_decode_error` (zero-byte / random bytes)
- `play_missing_file_returns_io_error`
- `cancel_without_active_play_is_noop`

### Integration tests (`tests/bundled_audio.rs`)

Using a `MockSpeaker` that counts `speak()` calls:
- `app_bundled_hit_skips_speaker`: bundle dir + s01-d1.mp3 present → mock count = 0
- `app_bundled_miss_falls_through_to_speaker`: no bundle dir → mock count = 1
- `app_bundled_partial_miss_falls_through_for_missing_drills`: only s01-d1.mp3 → navigating to s01-d2 → mock count ≥ 1
- `app_corrupt_bundle_does_not_call_speaker`: zero-byte mp3 → mock count = 0 (per §7 limitation)

### Fixture

`fixtures/audio/silence.mp3` — minimal decodable mp3 (~100 bytes, 50ms
silence at 16 kHz mono 16 kbps), pre-generated and committed. CI does not
require ffmpeg or a soundcard.

### Coverage target

- `bundle.rs`: 100% (pure functions + stat call)
- `player.rs`: ~80% (rodio Sink playback unobservable in CI)
- App integration: hit / miss / partial-miss / corrupt branches covered

## 9. Cargo Changes

`Cargo.toml`:
```toml
rodio = { version = "0.19", default-features = false, features = ["mp3"] }
```

Adds the `minimp3`-based decoder. No other dependency changes.

## 10. Out of Scope (v1)

- Bundle generation tooling (separate repo / external)
- Bundle integrity / version metadata
- Per-course voice config (the bundle's "voice" is implicit)
- Bundle-only mode (force-skip TTS even when fallback is available)
- Audio scrubbing UI / replay key
- Listening-only practice mode

## 11. Migration

None. Existing courses without bundle directories continue to work
unchanged via the fall-through path.
