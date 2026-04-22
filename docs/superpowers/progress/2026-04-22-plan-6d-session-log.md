# inkworm v1 Development Session Log — Plan 6d
**Date**: 2026-04-22
**Branch**: feat/v1-tts-integration (PR pending)

---

## Completed Work

### Plan 6d: TTS App Integration ✅

**Goal**: Wire `IflytekSpeaker` from Plan 6c into the running app so English drills actually speak as the user studies. Also persist the `rodio::Sink` inside `IflytekSpeaker` so `cancel()` stops playback mid-audio (Plan 6c's `mem::forget` made that impossible). Device detection, wizard TTS steps, and the `/tts` status overlay were deferred to Plan 6e.

**Implementation approach**: Subagent-driven development (6 tasks). Task 2 intentionally left the build broken (App::new signature changed but call-sites not yet updated); Tasks 3 and 4 closed it in sequence. No reviewer round-trips needed — controller-level verification between tasks was sufficient.

**Commits** (5 on feature branch):
1. `5bfcf5f` — feat(tts): persist rodio Sink on IflytekSpeaker so cancel stops playback
2. `4126d34` — feat(app): add speaker field and speak_current_drill drill-transition hook
3. `b379cc6` — feat(main): construct rodio OutputStream + speaker and pass to App
4. `2141707` — test: pass NullSpeaker to App::new in integration test helpers
5. `f09226c` — test(tts): integration tests for app-level speak/cancel on drill transitions

Plan doc already on `main` at `0b3157c docs: add Plan 6d TTS integration implementation plan`.

**Files changed** (vs. `0b3157c` baseline):
- `src/tts/iflytek.rs` — added `current_sink: Arc<Mutex<Option<rodio::Sink>>>`; `play_pcm` persists; `cancel` stops
- `src/app.rs` — `pub speaker: Arc<dyn Speaker>` field; `App::new` gains final `speaker` param; `speak_current_drill` method; 4 call sites (Tab skip, correct-advance, `/skip` palette, switch_to_course, GenerateProgress::Done)
- `src/main.rs` — `rodio::OutputStream::try_default()` + `build_speaker` + `Arc<dyn Speaker>` + startup `speak_current_drill`
- `tests/tts_palette.rs`, `tests/course_list.rs`, `tests/config_wizard.rs` — `make_app` helpers updated with `speaker: Arc::new(NullSpeaker)`
- `tests/course_list.rs` `switch_course_updates_active_and_returns_to_study` converted to `#[tokio::test]` (now needs a runtime because `speak_current_drill` uses `tokio::spawn`)
- `tests/tts_app_wiring.rs` — **new**, 150+ lines: `MockSpeaker` + 3 integration tests

**Test status**: 225 passing (222 baseline + 3 new integration). All test binaries green.

---

## New Capabilities

- Running `inkworm` now speaks the current drill's English via iFlytek + rodio whenever the user:
  - Launches the app with a loaded course (startup)
  - Presses Tab to skip a drill
  - Types correctly + hits Enter + presses any key to advance
  - Types `/skip` in the palette
  - Switches active course via `/list` → Enter
  - Finishes an `/import` generation
- `speaker.cancel()` now stops the WS stream AND halts the currently-playing Sink — no more audio overlap when drills change quickly.
- Audio device failures at startup (no speaker, CI, etc.) degrade silently to cache-only mode instead of crashing. An `eprintln!` note is printed during startup.

---

## Architecture Details

- `App::speak_current_drill()` is a public method on `App` that calls `self.speaker.cancel()` and, if `current_drill()` is `Some`, spawns `speaker.speak(english_text)` via `tokio::spawn`. Because `speaker` is `Arc<dyn Speaker>`, it clones cheaply and moves into the spawned future.
- `rodio::OutputStream` is constructed in `main` before entering the tokio runtime and held alive for the process lifetime (it's `!Send + !Sync`, so it must stay on the main thread). Only the `OutputStreamHandle` (which is `Send + Sync + Clone`) crosses into the speaker.
- `IflytekSpeaker::current_sink` is an `Arc<Mutex<Option<Sink>>>`. `play_pcm` stores the new sink (dropping any prior one — rodio's Drop doesn't stop playback, so the prior audio plays out unless `cancel` was called first, which is the expected flow). `cancel` takes the option out and calls `sink.stop()`.
- The `audio: Option<OutputStreamHandle>` pattern in `IflytekSpeaker` means tests and headless environments can construct a real speaker in cache-only mode with `audio=None` — no code branches for "is this a test"; just the audio handle's presence decides.

---

## Deviations from Plan

One small test change necessitated by the tokio::spawn addition:

1. **`tests/course_list.rs::switch_course_updates_active_and_returns_to_study`** had to become `#[tokio::test]` rather than plain `#[test]`. Reason: `speak_current_drill` calls `tokio::spawn` in all its downstream paths; without a runtime, the test panics with "there is no reactor running". Every test that drives drill transitions now needs a tokio runtime.

Other `tests/course_list.rs` tests don't trigger drill transitions directly (they only open the list or press Down / Esc), so they stayed sync. If future changes extend drill-transition coverage into those tests, they'll need the same conversion.

---

## Known Follow-ups (Plan 6e)

1. **Device detection** — `SwitchAudioSource` + `system_profiler` fallback, `OutputKind` classification, `should_speak(mode, device, creds)` decision. The App needs to consult this before calling `speak`; with Auto mode + built-in speaker, TTS is suppressed.
2. **1-second device-change tick** in the main event loop, pushing an `OutputKind` update into App state.
3. **Config wizard TTS steps** — extend `WizardState` / `WizardStep` enum with "Enable TTS? (y/n)" + (on y) app_id / api_key / api_secret entry. `validate_tts` already exists from Plan 1.
4. **`/tts` no-args status overlay** — show current mode, detected device, cache size, last error, creds configured yes/no. Similar pattern to Plan 5's `/list` overlay.
5. **3-strikes session-disable** per spec §7.6 — 3 consecutive speak() network failures → disable TTS for the session. Needs a failure counter on App or on a wrapper Speaker.
6. **AppError::Tts + user_message mapping** — current TTS errors are lost in the `let _ = speaker.speak(...).await;` in `speak_current_drill`. A status-line banner (or the `/tts` overlay) should surface the last one.
7. **Graceful shutdown** — on `quit`, we should probably `speaker.cancel()` before tearing down so the audio stops immediately. Currently it keeps playing a moment while the terminal restores.
8. **Tests that drive drill transitions must become `#[tokio::test]`** — call out in any contributor docs.

---

## Process Notes

- Worktree `../inkworm-tts-integration` created from main at `0b3157c`. Baseline 222 tests.
- Task 2 deliberately committed a broken build with `App::new` signature change; Tasks 3 and 4 closed it two commits later. This style — "change the interface, then fix each caller in a separate focused commit" — makes each commit individually reviewable, and the PR as a whole is a clean refactor.
- Every task used `rustfmt --edition 2021 --check <files>` instead of `cargo fmt --check`. Zero fmt-noise incidents this plan.
- Zero detached-HEAD incidents.
- `MockSpeaker` implementation (~15 lines + `#[async_trait]`) is a clean reusable pattern for any future Speaker-consumer tests. If Plan 6e grows more speaker interaction paths, this mock can migrate into `tests/common/`.
