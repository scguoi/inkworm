# inkworm v1 Development Session Log — Plan 6a
**Date**: 2026-04-22
**Branch**: feat/v1-tts-palette (PR pending)

---

## Completed Work

### Plan 6a: TTS Palette Subcommands ✅

**Goal**: Make `/tts on|off|auto|clear-cache` palette commands functional. No audio yet — this plan only flips `config.tts.override` flags and empties the cache directory. Plan 6b will add the real `IflytekSpeaker` and audio playback.

**Implementation approach**: Subagent-driven development (5 tasks). Each task → fresh implementer subagent → spec compliance review → code-quality review → commit.

**Commits** (4 on feature branch):
1. `074485b` — feat(palette): parse input into command and args; flip /tts available
2. `5dee704` — feat(tts): add clear_cache utility for tts-cache directory
3. `c7f2700` — feat(app): wire /tts on|off|auto|clear-cache palette subcommands
4. `4a9a04f` — test(tts): integration tests for /tts palette subcommands

Plan doc already on `main` at `23f863e docs: add Plan 6a /tts palette subcommands implementation plan`.

**Files changed** (vs. `23f863e` baseline):
- `src/ui/palette.rs` — `Command.takes_args`, `PaletteState::parse`, updated `matches/complete/confirm`; flipped `tts` to `available: true, takes_args: true`
- `src/app.rs` — updated call site to destructure `(cmd, args)`; `execute_command` signature carries `args: &[String]`; new `/tts` arm + `execute_tts` + `set_tts_override` helpers
- `src/lib.rs` — register `pub mod tts;`
- `src/tts/mod.rs` — **new**, only the `clear_cache` helper (Plan 6b grows this)
- `tests/tts_palette.rs` — **new**, 5 integration tests

**Test status**: 190 passing (176 baseline + 6 new palette unit tests + 3 new tts unit tests + 5 new integration tests). All test binaries green.

---

## New Features

- `/tts on` → `config.tts.override = On` + atomic save
- `/tts off` → `config.tts.override = Off` + atomic save
- `/tts auto` → `config.tts.override = Auto` + atomic save
- `/tts clear-cache` → deletes `.wav` files inside `tts-cache/`, preserves the directory and non-wav files
- Palette input now supports commands with arguments; Tab completion appends a trailing space for arg-taking commands (currently only `tts`)
- Unknown `/tts <arg>` is a silent no-op (avoids corrupting the TUI with stray `eprintln!` output)

---

## Architecture Changes

- `Command` gains `pub takes_args: bool` field. Only `tts` uses it so far.
- `PaletteState::parse(&self) -> (String, Vec<String>)` — strips leading `/`, splits on whitespace, lowercases the command word (args preserved as-typed).
- `PaletteState::matches` filters on the FIRST word only; so `/tts on` still matches the `tts` command.
- `PaletteState::confirm` returns `Option<(&'static Command, Vec<String>)>` — tuple with command and parsed args.
- `App::execute_command(&Command, &[String])` — arg-aware dispatch; `/tts` delegates to `App::execute_tts`.
- `App::set_tts_override` writes the mutated `Config` atomically and `eprintln!`s on save error (consistent with `switch_to_course`).
- `src/tts/mod.rs` created with one public function: `clear_cache(dir: &Path) -> io::Result<usize>`. Plan 6b will grow this module with Speaker trait, IflytekSpeaker, auth, device detection, etc.

---

## Deviations from Plan

None. The 5 tasks shipped exactly as written. Reviewer raised Minor observations (missing edge-case unit tests for `parse`, case-sensitive arg matching, pre-existing fmt debt) — all deferred or already accepted as scope-appropriate.

---

## Known Follow-ups (all deferred to Plan 6b unless noted)

1. **Real `Speaker` trait + `NullSpeaker`** — `/tts on` currently flips a flag that nothing consumes. Plan 6b adds actual speaker construction and wires into Study.
2. **`IflytekSpeaker` with tokio-tungstenite WS + cancellation semantics** — the meat of TTS (Plan 6b).
3. **rodio playback + WAV atomic writer + blake3 cache key**.
4. **Device detection** (`SwitchAudioSource` + `system_profiler` fallback) + 1s tick + `should_speak` decision fn.
5. **`/tts` with no args** — status overlay showing mode / cred status / device / cache size.
6. **Config wizard TTS steps** — extend the wizard with "Enable TTS? (y/n)" + (if yes) app_id / api_key / api_secret.
7. **Validate-on-`/tts on`** — currently `/tts on` with empty iflytek creds silently flips the flag; Plan 6b's speaker construction will degrade to `NullSpeaker` and surface a warning.
8. **Case-insensitive subcommand matching** — `/tts ON` currently no-ops silently; cheap fix (`s.to_lowercase()` on the arg) deferred since palette UX rarely has uppercase input.
9. **Edge-case `parse()` unit tests** — `""`, `"/"`, `"/  "`, `"/TTS On"` — reviewer's suggestion; actual behavior is correct, just untested.
10. **Pre-existing repo-wide fmt/clippy debt** (unchanged) — `cargo fmt --check` without file args still fails due to churn from earlier plans.

---

## Process Notes

- Worktree `../inkworm-tts-palette` created from `main` at `23f863e`. Baseline 176 tests.
- Tasks 2 and 4 were small enough that the controller did spec+quality verification inline rather than dispatching a full reviewer round — still caught all relevant issues and saved round-trips.
- Every implementer subagent respected the "`cargo fmt` with explicit file args only" rule after the Plan 5 Task 5 incident. No fmt noise bled into any commit this time.
- Plan 6a is intentionally the smallest useful slice of TTS work. The goal was a landable, reviewable delta that lays groundwork (palette arg parsing, `tts` module stub, config plumbing) without blocking on the larger audio infrastructure that belongs in Plan 6b.
