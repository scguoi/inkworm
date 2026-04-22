# inkworm v1 Development Session Log — Plan 6e
**Date**: 2026-04-22
**Branch**: feat/v1-tts-auto (PR pending)

---

## Completed Work

### Plan 6e: TTS Auto Mode (device detection) ✅

**Goal**: Make `config.tts.override = "auto"` actually work — probe the current macOS audio output every ~1s, classify it, and suppress TTS on built-in speakers / external displays / unknown devices. Ship the decision fn `should_speak(mode, device, has_creds)` from spec §7.5 and plumb it through `speak_current_drill`.

**Implementation approach**: Subagent-driven, 5 tasks (Plan 6e originally had 6; Task 2 was a `Box::leak` cleanup that the controller folded into Task 1, so the implementation shipped a clean `Option<String>` heading tracker from the start).

**Commits** (4 on feature branch):
1. `2ca84ee` — feat(tts): add OutputKind classification + should_speak decision fn
2. `b19f23d` — feat(app): add device-probe tick and current_device state
3. `4c2ef72` — feat(app): gate speak_current_drill on should_speak decision *(intentionally broke 3 MockSpeaker tests; next commit fixes)*
4. `3392567` — test(tts): force override=On + creds so should_speak gate passes

Plan doc already on `main` at `1a8759e docs: add Plan 6e TTS auto-mode implementation plan`.

**Files changed** (vs. `1a8759e` baseline):
- `src/tts/mod.rs` — register `pub mod device;`, re-export `OutputKind` + `should_speak`
- `src/tts/device.rs` — **new**, ~160 lines: `OutputKind` enum, pure `classify(name)` rule table, `should_speak(mode, device, has_creds)`, `detect_output_kind()` with `SwitchAudioSource` → `system_profiler` fallback, 12 unit tests
- `src/ui/task_msg.rs` — add `TaskMsg::DeviceDetected(OutputKind)` variant
- `src/app.rs` — `OutputKind` import, `current_device` + `device_probe_counter` fields, `on_tick` spawns probe every 62 ticks via `tokio::task::spawn_blocking`, `on_task_msg` handles `DeviceDetected`, `tts_has_creds` helper, `speak_current_drill` gated on `should_speak`
- `tests/tts_app_wiring.rs` — `make_app` helper sets `TtsOverride::On` + fills `test-app` / `test-key` / `test-secret` so the new gate passes regardless of probed device

**Test status**: 237 passing (225 baseline + 12 new device tests). All test binaries green.

---

## New Capabilities

- At startup and every ~1s thereafter, inkworm probes the current macOS audio output using:
  1. `SwitchAudioSource -c -t output` (brew-installable; preferred because output is a clean single-line device name)
  2. `system_profiler SPAudioDataType` (always available; the parser walks 4-space-indented `DeviceName:` headings and returns the one sitting above "Default Output Device: Yes")
- The probe runs off the main thread via `tokio::task::spawn_blocking`. On any failure (tool not found, command error, empty output), it returns `OutputKind::Unknown` — never panics, never propagates IO errors to the UI.
- Classification rules (case-insensitive, order-sensitive):
  - `airpods` / `bluetooth` / `beats` → `Bluetooth`
  - `headphone` / `earphone` / `headset` → `WiredHeadphones`
  - `macbook` + `speaker` → `BuiltInSpeaker`
  - `display` / `hdmi` → `ExternalSpeaker`
  - anything else → `Unknown`
- Decision (`should_speak(mode, device, has_creds)`):
  - no creds → false
  - `On` → true (with creds)
  - `Off` → false
  - `Auto` → true only on `Bluetooth` / `WiredHeadphones`; silent on everything else, *including* `Unknown`
- `speak_current_drill` consults `should_speak` before spawning, so Auto mode automatically silences on built-in speakers and external displays. Cancel remains unconditional — disabling TTS mid-drill stops any in-flight audio immediately.

---

## Architecture Notes

- The 1s probe cadence is implemented as a counter inside the existing 16ms tick rather than an independent interval — it reuses the established `on_tick` surface and avoids adding a second timer to the event loop. Practical cadence is ~1 Hz (62 × 16 ms = 992 ms).
- `OutputKind::Unknown` is the safe default for Auto mode: if we can't classify a device, we don't speak. A user who has headphones connected but runs on a system without `SwitchAudioSource` and whose `system_profiler` output doesn't match our patterns will get silence in Auto mode — they can always flip to `/tts on` to force playback.
- `detect_output_kind()` is unit-tested only to the extent that it never errors out. The classify rules are the interesting logic and get their own 8 table-driven tests. True integration coverage (actual `switchaudiosource` binary producing specific device names) would need a device-fixture subsystem — deferred as unnecessary for v1 given the fallback semantics.

---

## Deviations from Plan

1. **Plan Task 2 folded into Task 1**: the written plan had Task 1 use `Box::leak` to bridge borrow lifetimes in `try_system_profiler`, then Task 2 cleanup it up. Controller rewrote Task 1's prompt to skip the dirty intermediate, so `try_system_profiler` shipped with an owned `Option<String>` heading tracker from commit one. No Box::leak ever landed in the repo.
2. **Unit test count**: plan said 11, implementer wrote 12 (an extra sanity assertion in the Unknown path). Trivial.

Otherwise the 5 tasks shipped as written.

---

## Known Follow-ups

**Plan 6f (UX polish for TTS):**
- Extend config wizard with 4 more steps (enable y/n → app_id → api_key → api_secret). Plan 4b's wizard already has `validate_tts` from Plan 1 — just need to add the UI flow.
- `/tts` no-args status overlay: show current mode, `current_device`, cache size, creds-set y/n, last error. Similar pattern to Plan 5's `/list` overlay.

**Plan 7 (robustness + polish):**
- `/logs` command (per spec §8.3): show log-file path + `pbcopy`.
- `/doctor` command: health check — LLM reachable, iFlytek reachable, cache dir writable, audio device present.
- Tracing / log-file wiring per spec §10.4: `~/.config/inkworm/inkworm.log`; `INKWORM_LOG=debug` override; never log api keys or article text.
- 3-strikes session-disable per spec §7.6: 3 consecutive speak() network failures → suppress TTS for the session (log once, surface via `/tts` status overlay).
- Graceful `speaker.cancel()` on `quit` so audio stops immediately instead of trailing during terminal restoration.
- `AppError::Tts` variant + `user_message` mapping: current TTS errors are lost in `let _ = speaker.speak(...).await;`. Needs a status-line or banner path.

---

## Process Notes

- Worktree `../inkworm-tts-auto` created from main at `1a8759e`. Baseline 225.
- Every task used `rustfmt --edition 2021 --check <files>` for per-file checks. Zero fmt-noise incidents — the memory entry keeps paying dividends.
- Task 3 commit is intentionally a "broken build" in the sense that `tts_app_wiring`'s 3 tests fail between Task 3 and Task 5. Each commit is individually reviewable; the PR is a clean refactor overall.
- Controller caught one Plan authoring mistake pre-flight (Task 2's Box::leak step was wasteful) and folded the cleanup into Task 1. Worth doing more upfront scoping in later plans — smaller, correct steps beat two-step ugly-then-clean sequences.
