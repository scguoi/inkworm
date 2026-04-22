# inkworm v1 Development Session Log — Plan 6b
**Date**: 2026-04-22
**Branch**: feat/v1-tts-core (PR pending)

---

## Completed Work

### Plan 6b: TTS Core (pure logic) ✅

**Goal**: Land the deterministic half of the TTS subsystem — iFlytek URL signing, blake3 cache key + path helpers, WAV atomic I/O, and the `Speaker` trait with a `NullSpeaker` fallback. Everything shipped here is unit-testable with zero network and zero audio hardware.

**Implementation approach**: Subagent-driven development (6 tasks). Each task → fresh implementer subagent → controller verification / spec review → commit. Two small fixes landed as separate commits.

**Commits** (6 on feature branch, 7 with plan doc already on main):
1. `39dd3a5` — build(deps): add hmac/sha2/base64/urlencoding/httpdate/blake3/hound for TTS core
2. `609975b` — feat(tts): add iFlytek URL signing with insta snapshot test
3. `30360b3` — feat(tts): add blake3-based cache key and path helpers
4. `e889332` — feat(tts): add atomic WAV writer and reader for cache
5. `5514bf9` — refactor(tts): fsync tmp wav file before rename
6. `1eea1e6` — feat(tts): add Speaker trait, TtsError, NullSpeaker, and build_speaker factory

Plan doc already on `main` at `4e44fc1 docs: add Plan 6b TTS core implementation plan`.

**Files changed** (vs. `4e44fc1` baseline):
- `Cargo.toml`, `Cargo.lock` — 7 new deps (`hmac`, `sha2`, `base64`, `urlencoding`, `httpdate`, `blake3`, `hound`)
- `src/tts/mod.rs` — register 4 new submodules, re-export Speaker API
- `src/tts/auth.rs` — **new**, ~60 lines: `build_authorized_url` + `hmac_sha256_base64` helper + 4 tests (including insta snapshot)
- `src/tts/cache.rs` — **new**, ~60 lines: `cache_key` + `cache_path` + 6 tests
- `src/tts/wav.rs` — **new**, ~130 lines: `write_wav_atomic` + `read_wav_pcm` + 4 tests; mirrors `storage::atomic::write_atomic` fsync semantics
- `src/tts/speaker.rs` — **new**, ~140 lines: `Speaker` trait, `TtsError` enum, `NullSpeaker`, `build_speaker` factory + 6 tests
- `src/tts/snapshots/inkworm__tts__auth__tests__authorized_url_snapshot.snap` — **new**, insta-generated

**Test status**: 210 passing (190 baseline + 4 auth + 6 cache + 4 wav + 6 speaker). All test binaries green.

---

## New Capabilities

- `tts::auth::build_authorized_url(api_key, api_secret, now)` — pure fn producing the signed wss:// URL per iFlytek's HMAC-SHA256 scheme; deterministic with a fixed clock.
- `tts::cache::cache_key(text, voice)` — 64-hex-char blake3 hash; newline separator prevents `"abc"+"def"` vs. `"ab"+"cdef"` collisions.
- `tts::cache::cache_path(dir, key)` — builds `<dir>/<key>.wav`.
- `tts::wav::{write_wav_atomic, read_wav_pcm}` — atomic (tmp-file + fsync + rename + dir fsync) WAV I/O for mono 16-bit 16kHz PCM; rejects mismatched specs on read.
- `tts::speaker::{Speaker, TtsError, NullSpeaker, build_speaker}` — trait + no-op impl + factory that returns `NullSpeaker` when creds are missing or `TtsOverride::Off`. Plan 6c will extend `build_speaker` to return `IflytekSpeaker` otherwise.

---

## Deviations from Plan

Two small corrections surfaced during implementation:

1. **WAV atomic-write fsync (Task 4)**: The plan's code assumed `hound::WavWriter::finalize()` returns the underlying `File`, so it called `sync_all()` on the returned file. In `hound` 3.5, `finalize()` returns `Result<()>` and consumes the writer. The first Task 4 commit (`e889332`) silently dropped the file-level fsync; the follow-up `5514bf9` restored it by re-opening the tmp path and calling `sync_all` on the fresh handle before rename, matching `storage::atomic::write_atomic` semantics.

2. **`io::Error::new(ErrorKind::Other, ...)` → `io::Error::other(...)`**: Clippy flagged the verbose form; implementer used the shorter API throughout `wav.rs`. Purely stylistic.

---

## Known Follow-ups (Plan 6c / 6d)

1. **`IflytekSpeaker`** — tokio-tungstenite WS loop, cancellation token via `stream_handle: Arc<Mutex<Option<CancellationToken>>>`, PCM accumulator, rodio sink (Plan 6c).
2. **App integration** — construct speaker in `main.rs`, wire into `StudyState::advance` to speak English drill, cancel on drill change, 1s device-probe tick (Plan 6c).
3. **Device detection** — `SwitchAudioSource` + `system_profiler` fallback, `OutputKind` classification, `should_speak(mode, device, has_creds)` decision fn (Plan 6d).
4. **Config wizard TTS step** — extend wizard with "Enable TTS? (y/n)" + (if yes) app_id / api_key / api_secret entry (Plan 6d).
5. **`/tts` no-args status overlay** — display mode / device / cache size / last error (Plan 6d).

---

## Process Notes

- Worktree `../inkworm-tts-core` created from `main` at `4e44fc1`. Baseline 190 tests.
- **`cargo fmt --check -- <files>` ignores file args.** Surfaced during Task 5 when the implementer tried to scope a fmt check and `cargo fmt` walked the whole workspace anyway. Workaround: use `rustfmt --edition 2021 --check <files>` for per-file checks. `cargo fmt -- <files>` (without `--check`) still respects the file list. Added to project memory.
- Every implementer ran `cargo fmt` at some point during their task; Tasks 1-4 each contaminated the working tree with pre-existing fmt churn, which the controller cleaned up via `git checkout HEAD -- <files>` before the next task. No fmt noise made it into any commit after Task 4's fix round.
- Subagent detaching-HEAD was NOT an issue this plan — all 6 commits landed on the attached `feat/v1-tts-core` branch cleanly.
- Plan scope held: six tasks shipped exactly as written modulo the two small `wav.rs` corrections noted above.
