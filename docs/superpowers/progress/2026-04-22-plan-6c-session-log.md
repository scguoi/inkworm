# inkworm v1 Development Session Log — Plan 6c
**Date**: 2026-04-22
**Branch**: feat/v1-tts-live (PR pending)

---

## Completed Work

### Plan 6c: Live IflytekSpeaker (WS + rodio) ✅

**Goal**: Replace Plan 6b's `NullSpeaker` placeholder with a real `IflytekSpeaker` that opens an authorized WS to `tts-api.xfyun.cn`, streams PCM chunks, writes a WAV cache, and optionally plays via rodio. Shipped in isolation — App integration deferred to Plan 6d.

**Implementation approach**: Subagent-driven development (7 tasks). Each task → fresh implementer subagent → controller verification → commit. No spec-reviewer round trips this plan; controller-level scope checks were sufficient given the plan's code was largely complete.

**Commits** (6 on feature branch):
1. `56c1741` — build(deps): add tokio-tungstenite and rodio for live TTS
2. `6b4f39e` — feat(tts): add iFlytek request/response frame types and PCM decoder
3. `146bf4e` — feat(tts): add IflytekSpeaker scaffold with cache-hit path and cancel plumbing
4. `063aca4` — feat(tts): implement iFlytek WS streaming with cancellation-aware select loop
5. `53160c0` — feat(tts): build_speaker returns IflytekSpeaker when creds present
6. `1496acc` — test(tts): integration tests for IflytekSpeaker with mock WS server

Plan doc already on `main` at `d40f885 docs: add Plan 6c live TTS implementation plan`.

**Files changed** (vs. `d40f885` baseline):
- `Cargo.toml`, `Cargo.lock` — 2 new deps (`tokio-tungstenite`, `rodio`)
- `src/tts/mod.rs` — register `frame` + `iflytek` submodules; re-export `IflytekSpeaker`
- `src/tts/frame.rs` — **new**, ~170 lines: request/response serde structs, `build_request_frame`, `parse_response`, `decode_pcm`, 6 unit tests
- `src/tts/iflytek.rs` — **new**, ~240 lines: `IflytekSpeaker`, cache-hit path, WS streaming with `tokio::select!` cancel, `classify_iflytek_code` helper, 3 unit tests
- `src/tts/speaker.rs` — `build_speaker` now takes `Option<rodio::OutputStreamHandle>` and returns `IflytekSpeaker` when creds present + mode ≠ Off
- `tests/iflytek_speaker.rs` — **new**, ~140 lines: 3 integration tests against a localhost tokio-tungstenite mock server

**Test status**: 222 passing (210 baseline + 6 frame + 3 iflytek + 3 integration). All test binaries green.

---

## New Capabilities

- `tts::frame::{build_request_frame, parse_response, decode_pcm}` — pure JSON + base64 helpers for iFlytek's one-shot-request / streamed-response protocol.
- `tts::IflytekSpeaker::new(cfg, cache_dir, audio)` — production constructor; `audio = None` = cache-only (tests + headless).
- `tts::IflytekSpeaker::with_base_url(cfg, cache_dir, audio, base_url)` — test-only constructor for local `ws://` mocks; tolerates any signature (mock ignores auth).
- `IflytekSpeaker::speak(text)`:
  - cache hit → read WAV via `hound`, queue playback (if audio handle), return Ok
  - cache miss → store fresh `CancellationToken` in `stream_handle`, open WS via `tokio-tungstenite::connect_async`, send one request frame, collect response frames under `tokio::select!` (biased on cancel), accumulate PCM, write WAV cache, queue playback
- `IflytekSpeaker::cancel()` — pulls the active token out of `stream_handle` and cancels; the `select!` loop surfaces `TtsError::Cancelled` and the cache write is skipped.
- `build_speaker(cfg, cache_dir, mode, audio)` — returns `IflytekSpeaker` when creds present + mode ≠ Off; `NullSpeaker` otherwise.

---

## Deviations from Plan

One test-vector correction:

1. **`decode_pcm_odd_byte_count_is_error` (Task 2)**: plan claimed `"AAA="` base64-decodes to 3 bytes. Correct value is 2 bytes — base64's `=` padding indicates 1 byte of discard, not add. Implementer used `"AQID"` (decodes to 3 bytes, hitting the odd-byte-count error path).

Other than that, all 6 tasks shipped as written. No re-review loops needed.

---

## Known Follow-ups (Plan 6d)

1. **App integration** — `main.rs` constructs `rodio::OutputStream` (kept alive for the process lifetime) + pulls `OutputStreamHandle`, calls `build_speaker`, stores `Arc<dyn Speaker>` on `App`. `StudyState::advance` (or the App's key handler that triggers it) invokes `speaker.speak(english)` via `tokio::spawn` and calls `speaker.cancel()` when the drill changes / user skips / drill is completed correctly.
2. **Persisted `Sink` for mid-playback cancel** — current `play_pcm` uses `mem::forget(sink)` to detach; `cancel` can't stop the rodio side mid-playback. Plan 6d introduces `IflytekSpeaker::current_sink: Mutex<Option<Sink>>` so `cancel` can call `sink.stop()` too. Requires a small Speaker state refactor.
3. **Device detection** — `SwitchAudioSource` + `system_profiler` fallback, `OutputKind` classification, `should_speak(mode, device, has_creds)` decision. 1-second tick in the main event loop.
4. **Config wizard TTS step** — "Enable TTS? (y/n)" + (if yes) app_id / api_key / api_secret entry. Probably 3-4 new wizard steps; `validate_tts` already exists.
5. **`/tts` no-args status overlay** — shows current mode, device kind, cache size, last error. Similar pattern to the Plan 5 `/list` overlay.
6. **3-strikes session disable** — per spec §7.6, 3 consecutive WS failures should disable TTS for the session (Speaker falls back to Null internally or at the App level).
7. **AppError::Tts variant + user_message mapping** — `TtsError` currently surfaces via `eprintln!` in `speak`; Plan 6d adds a proper banner path.

---

## Process Notes

- Worktree `../inkworm-tts-live` created from `main` at `d40f885`. Baseline 210.
- Every task used `rustfmt --edition 2021 --check <files>` instead of `cargo fmt --check` — zero fmt-noise incidents. The memory entry from Plan 6b paid off.
- Every implementer stayed on the attached `feat/v1-tts-live` branch — no detached-HEAD incidents.
- Plan length was ~1077 lines (second only to Plan 5). Code blocks were fully spelled out, which kept implementer subagents on track; only one real deviation (the base64 test vector) throughout 6 tasks.
- Integration test's mock WS server is ~25 lines and reuses `tokio-tungstenite::accept_async` with a throwaway `TcpListener::bind("127.0.0.1:0")`. Reusable pattern for any future WS-adjacent tests (Plan 6d won't need it, but 6c's pattern can migrate into `tests/common/` if we grow more WS fixtures).
