# Plan 6c: IflytekSpeaker (live WS + rodio playback) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `NullSpeaker` returned by Plan 6b's `build_speaker` with a real `IflytekSpeaker` that opens an authorized WS to `tts-api.xfyun.cn`, streams PCM chunks, caches the result as a WAV, and optionally plays via rodio. Plan 6d wires the speaker into `App`, adds device detection, wizard TTS steps, and the `/tts` status overlay — this plan ships the speaker in isolation.

**Architecture:** `IflytekSpeaker` owns: (a) an `Arc<Mutex<Option<CancellationToken>>>` so `cancel()` can interrupt whatever `speak()` is currently doing; (b) an optional `rodio::OutputStreamHandle` — when `None`, the speaker runs in "cache-only" mode (used by tests and by servers without audio hardware). `speak(text)` computes the cache key; on hit, reads the WAV via `hound` and queues playback (if handle); on miss, opens a WS to the URL from `auth::build_authorized_url`, sends one request frame, collects response frames until `status==2`, writes the WAV cache, and queues playback. A test-only constructor `IflytekSpeaker::with_base_url` lets integration tests point at a local `ws://` mock server.

**Tech Stack:** Rust · `tokio-tungstenite` 0.24 (rustls-tls-webpki-roots) · `rodio` 0.19 (default-features off, only `rodio::Sink` + `SamplesBuffer` path) · existing `blake3`, `hound`, `base64`, `async-trait`, `tokio-util::CancellationToken`, `serde_json`.

---

## Scope & Non-Goals

**In scope (this plan):**
- Add deps `tokio-tungstenite` and `rodio`.
- `src/tts/frame.rs` — request-frame builder JSON (status=2 one-shot), response-frame shape, base64 PCM decode helper.
- `src/tts/iflytek.rs` — `IflytekSpeaker` struct + `Speaker` impl with cache-hit and cache-miss paths, cancellation, optional rodio playback.
- `build_speaker` factory gains an `Option<rodio::OutputStreamHandle>` parameter; when creds present and mode ≠ Off it constructs an `IflytekSpeaker`.
- Integration test `tests/iflytek_speaker.rs` with a tokio-tungstenite mock server on `127.0.0.1`.

**Out of scope (deferred to Plan 6d):**
- App integration — constructing speaker in `main.rs`, storing on `App`, invoking on drill advance / cancel.
- Device detection (`SwitchAudioSource` / `system_profiler`), `OutputKind`, `should_speak` decision fn.
- Config wizard TTS step (app_id / api_key / api_secret entry).
- `/tts` no-args status overlay.
- Session-level error-degradation (3 consecutive failures → disable-for-session).

**Out of scope entirely (v1 won't ship):**
- Non-iFlytek engines.
- Streaming partial-sentence playback chunks (we collect all, then play — simpler, slightly higher first-audio latency).

---

## File Structure

- **Modify** `Cargo.toml` — add `tokio-tungstenite` and `rodio` under `[dependencies]`.
- **Modify** `src/tts/mod.rs` — register `frame` and `iflytek` submodules; re-export `IflytekSpeaker`.
- **Create** `src/tts/frame.rs` — ~100 lines: `RequestFrame` (Serialize), `ResponseFrame` (Deserialize), `decode_pcm` helper, tests.
- **Create** `src/tts/iflytek.rs` — ~250 lines: `IflytekSpeaker` struct, `Speaker` trait impl, cache path, WS loop, cancellation, optional rodio playback, unit tests for cache-hit path.
- **Modify** `src/tts/speaker.rs` — `build_speaker` gains an `audio: Option<rodio::OutputStreamHandle>` parameter; returns `IflytekSpeaker` when creds + mode check pass.
- **Create** `tests/iflytek_speaker.rs` — integration test: binds a localhost TCP listener, accepts one WS connection, replays a canned 3-frame response, asserts cache file written and speaker returns Ok.

---

## Pre-Task Setup

- [ ] **Setup 0.1: Verify clean main and create worktree**

```bash
cd /Users/scguo/.tries/2026-04-21-scguoi-inkworm
git status                              # clean on main
git log --oneline -3                    # HEAD is 7a4077f (Plan 6b merge)
git worktree add -b feat/v1-tts-live ../inkworm-tts-live main
cd ../inkworm-tts-live
cargo test --all                        # baseline 210 passing
```

Expected: worktree created; 210 tests green.

---

## Task 1: Add `tokio-tungstenite` + `rodio` dependencies

**Files:** `Cargo.toml`

- [ ] **Step 1.1: Append deps**

Inside the existing `[dependencies]` block, append:

```toml
# TTS live (Plan 6c): WS streaming + local audio playback
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
rodio = { version = "0.19", default-features = false }
```

`rodio` with `default-features = false` skips all decoder features — we only use `SamplesBuffer` for raw PCM playback; WAV decoding goes through our existing `hound`.

- [ ] **Step 1.2: Verify resolution**

```bash
cd /Users/scguo/.tries/inkworm-tts-live
cargo check
```

Expected: both crates resolve; `inkworm` compiles. (First build will pull in a few dozen transitive crates including `tungstenite`, `rustls`, `cpal` — it takes 30-60s.)

- [ ] **Step 1.3: Run existing tests**

```bash
cargo test --all 2>&1 | grep "test result" | awk '{sum+=$4} END {print "total:", sum}'
```

Expected: `total: 210` (unchanged).

- [ ] **Step 1.4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build(deps): add tokio-tungstenite and rodio for live TTS"
```

---

## Task 2: `frame.rs` — request/response frame types + PCM decode

**Files:**
- Modify: `src/tts/mod.rs`
- Create: `src/tts/frame.rs`

- [ ] **Step 2.1: Register submodule**

Append to `src/tts/mod.rs` (after the existing `pub mod speaker;` line):

```rust
pub mod frame;
```

- [ ] **Step 2.2: Create `src/tts/frame.rs`**

iFlytek's TTS online WS protocol (see their developer docs): one JSON request frame with `status: 2` (one-shot), followed by streamed JSON response frames with `data.audio` (base64 PCM) and `data.status` 0 (first), 1 (continuing), 2 (final).

Create `src/tts/frame.rs` with:

```rust
//! iFlytek TTS online WS request/response frames (per developer docs).
//! Request is one-shot (`data.status = 2`); response is streamed with
//! `data.status` 0 (first chunk), 1 (continuing), 2 (final chunk).

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct RequestFrame<'a> {
    pub common: RequestCommon<'a>,
    pub business: RequestBusiness<'a>,
    pub data: RequestData,
}

#[derive(Debug, Serialize)]
pub struct RequestCommon<'a> {
    pub app_id: &'a str,
}

#[derive(Debug, Serialize)]
pub struct RequestBusiness<'a> {
    pub aue: &'a str,  // "raw" = 16kHz 16-bit mono PCM
    pub vcn: &'a str,  // voice code, e.g. "x3_catherine"
    pub tte: &'a str,  // text encoding, "UTF8"
}

#[derive(Debug, Serialize)]
pub struct RequestData {
    pub status: u8,   // 2 = only-and-final frame
    pub text: String, // base64-encoded UTF-8 text
}

/// Build the one-shot request frame JSON string for the given text + voice + app id.
pub fn build_request_frame(app_id: &str, voice: &str, text: &str) -> String {
    let frame = RequestFrame {
        common: RequestCommon { app_id },
        business: RequestBusiness {
            aue: "raw",
            vcn: voice,
            tte: "UTF8",
        },
        data: RequestData {
            status: 2,
            text: BASE64.encode(text.as_bytes()),
        },
    };
    serde_json::to_string(&frame).expect("RequestFrame always serializes")
}

#[derive(Debug, Deserialize)]
pub struct ResponseFrame {
    pub code: i32,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub sid: String,
    pub data: Option<ResponseData>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseData {
    pub status: u8,          // 0=first, 1=continuing, 2=final
    #[serde(default)]
    pub audio: String,        // base64 PCM chunk (may be empty on some frames)
    #[serde(default)]
    pub ced: String,          // opaque progress counter (ignored)
}

impl ResponseFrame {
    pub fn is_ok(&self) -> bool {
        self.code == 0
    }
    pub fn is_final(&self) -> bool {
        matches!(self.data.as_ref().map(|d| d.status), Some(2))
    }
}

/// Decode a response frame's base64 `audio` field into raw i16 PCM samples (LE).
/// Returns `Ok(vec![])` for empty audio payloads (some frames carry no audio).
pub fn decode_pcm(b64: &str) -> Result<Vec<i16>, FrameError> {
    if b64.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = BASE64.decode(b64).map_err(|e| FrameError::Base64(e.to_string()))?;
    if bytes.len() % 2 != 0 {
        return Err(FrameError::Base64(format!(
            "odd byte count: {}",
            bytes.len()
        )));
    }
    let mut samples = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        samples.push(i16::from_le_bytes([pair[0], pair[1]]));
    }
    Ok(samples)
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("base64 decode: {0}")]
    Base64(String),
    #[error("json: {0}")]
    Json(String),
}

/// Parse a raw WS text-frame payload into a `ResponseFrame`.
pub fn parse_response(text: &str) -> Result<ResponseFrame, FrameError> {
    serde_json::from_str(text).map_err(|e| FrameError::Json(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_frame_is_valid_json_with_base64_text() {
        let raw = build_request_frame("app-xxx", "x3_catherine", "Hello!");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["common"]["app_id"], "app-xxx");
        assert_eq!(v["business"]["aue"], "raw");
        assert_eq!(v["business"]["vcn"], "x3_catherine");
        assert_eq!(v["business"]["tte"], "UTF8");
        assert_eq!(v["data"]["status"], 2);
        let b64 = v["data"]["text"].as_str().unwrap();
        let decoded = BASE64.decode(b64).unwrap();
        assert_eq!(decoded, b"Hello!");
    }

    #[test]
    fn parse_response_ok_with_final_flag() {
        let raw = r#"{"code":0,"message":"success","sid":"x","data":{"status":2,"audio":"","ced":"0"}}"#;
        let f = parse_response(raw).unwrap();
        assert!(f.is_ok());
        assert!(f.is_final());
    }

    #[test]
    fn parse_response_error_code_surfaces() {
        let raw = r#"{"code":10105,"message":"auth failed","sid":"x"}"#;
        let f = parse_response(raw).unwrap();
        assert!(!f.is_ok());
        assert_eq!(f.code, 10105);
        assert_eq!(f.message, "auth failed");
        assert!(f.data.is_none());
    }

    #[test]
    fn decode_pcm_empty_returns_empty() {
        assert_eq!(decode_pcm("").unwrap(), Vec::<i16>::new());
    }

    #[test]
    fn decode_pcm_round_trip() {
        let samples: Vec<i16> = vec![0, 256, -256, 32767, -32768];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let b64 = BASE64.encode(bytes);
        let decoded = decode_pcm(&b64).unwrap();
        assert_eq!(decoded, samples);
    }

    #[test]
    fn decode_pcm_odd_byte_count_is_error() {
        // base64 of 3 bytes = "AAA=" decoded to 3 bytes — odd
        let err = decode_pcm("AAA=").unwrap_err();
        assert!(matches!(err, FrameError::Base64(_)));
    }
}
```

- [ ] **Step 2.3: Run tests + rustfmt**

```bash
cd /Users/scguo/.tries/inkworm-tts-live
cargo test --lib tts::frame
rustfmt --edition 2021 --check src/tts/mod.rs src/tts/frame.rs
```

Expected: 5 tests pass; rustfmt silent.

- [ ] **Step 2.4: Commit**

```bash
git add src/tts/mod.rs src/tts/frame.rs
git commit -m "feat(tts): add iFlytek request/response frame types and PCM decoder"
```

---

## Task 3: `IflytekSpeaker` scaffold + cache-hit path (no WS yet)

**Files:**
- Modify: `src/tts/mod.rs`
- Create: `src/tts/iflytek.rs`

- [ ] **Step 3.1: Register submodule and re-export**

Update `src/tts/mod.rs`. Append:

```rust
pub mod iflytek;

pub use iflytek::IflytekSpeaker;
```

- [ ] **Step 3.2: Create `src/tts/iflytek.rs` scaffold**

This task lands the struct, constructor, and the cache-hit branch of `speak`. The WS branch lands in Task 4; cancellation in Task 5; rodio in Task 6.

```rust
//! Live iFlytek TTS speaker — WS streaming + WAV cache + optional rodio playback.
//!
//! Tasks 4/5/6 progressively fill in: Task 4 the WS miss path, Task 5 cancellation
//! via `stream_handle`, Task 6 rodio playback hooks.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::config::IflytekConfig;
use crate::tts::speaker::{Speaker, TtsError};
use crate::tts::{cache, wav};

const DEFAULT_BASE_URL: &str = "wss://tts-api.xfyun.cn/v2/tts";

pub struct IflytekSpeaker {
    cfg: IflytekConfig,
    cache_dir: PathBuf,
    /// Set by `speak`, read by `cancel`. `Mutex<Option<...>>` so `cancel`
    /// can pull out the active token with only `&self`.
    stream_handle: Arc<Mutex<Option<CancellationToken>>>,
    /// `None` = cache-only mode (tests, headless servers). Plan 6d's App
    /// integration will usually pass `Some(handle)`.
    audio: Option<rodio::OutputStreamHandle>,
    /// Overridden by `with_base_url` in tests.
    base_url: String,
}

impl IflytekSpeaker {
    pub fn new(
        cfg: IflytekConfig,
        cache_dir: PathBuf,
        audio: Option<rodio::OutputStreamHandle>,
    ) -> Self {
        Self {
            cfg,
            cache_dir,
            stream_handle: Arc::new(Mutex::new(None)),
            audio,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Test-only constructor: point the speaker at a local `ws://` mock.
    pub fn with_base_url(
        cfg: IflytekConfig,
        cache_dir: PathBuf,
        audio: Option<rodio::OutputStreamHandle>,
        base_url: String,
    ) -> Self {
        Self {
            cfg,
            cache_dir,
            stream_handle: Arc::new(Mutex::new(None)),
            audio,
            base_url,
        }
    }

    fn cache_path_for(&self, text: &str) -> PathBuf {
        let key = cache::cache_key(text, &self.cfg.voice);
        cache::cache_path(&self.cache_dir, &key)
    }

    /// Queue samples for playback via rodio, if an audio handle is present.
    /// Returns Ok(()) (no-op) when `self.audio` is None.
    fn play_pcm(&self, samples: Vec<i16>) -> Result<(), TtsError> {
        let Some(handle) = &self.audio else { return Ok(()) };
        let sink = rodio::Sink::try_new(handle)
            .map_err(|e| TtsError::Audio(e.to_string()))?;
        sink.append(rodio::buffer::SamplesBuffer::new(
            wav::CHANNELS,
            wav::SAMPLE_RATE,
            samples,
        ));
        // Detach: `sink` is moved into an internal thread; dropping it here
        // does NOT stop playback (rodio keeps playing until the buffer ends
        // or `sink.stop()` is called). The returned `Sink` handle is the
        // only way to stop mid-playback; we intentionally drop it for v1 and
        // let the WS cancel path stop playback on the rodio side via its
        // own Sink when we refactor in Task 6.
        std::mem::forget(sink);
        Ok(())
    }

    /// Also used by Task 4's WS miss path — suppress unused warnings until then.
    #[allow(dead_code)]
    fn authorized_url(&self, now: SystemTime) -> String {
        // Reuse Plan 6b's pure signing fn. Only difference in tests: the
        // `base_url` is a local ws:// URL; signing against localhost won't
        // pass iFlytek auth but the mock server ignores the header anyway.
        if self.base_url == DEFAULT_BASE_URL {
            crate::tts::auth::build_authorized_url(&self.cfg.api_key, &self.cfg.api_secret, now)
        } else {
            // Test path: append dummy query params so request shape is similar.
            format!("{}?test=1", self.base_url)
        }
    }
}

#[async_trait]
impl Speaker for IflytekSpeaker {
    async fn speak(&self, text: &str) -> Result<(), TtsError> {
        let path = self.cache_path_for(text);
        // Cache hit: read WAV, queue playback, done.
        if path.exists() {
            let samples = wav::read_wav_pcm(&path)
                .map_err(|e| TtsError::Cache(format!("cache read: {e}")))?;
            return self.play_pcm(samples);
        }
        // Cache miss: Task 4 fills this in. For now, return MissingCreds-like
        // error so tests written against this scaffold fail loudly rather
        // than silently "succeed".
        Err(TtsError::Network("WS path not implemented yet (Task 4)".into()))
    }

    fn cancel(&self) {
        if let Ok(mut guard) = self.stream_handle.lock() {
            if let Some(token) = guard.take() {
                token.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::wav;

    fn dummy_cfg() -> IflytekConfig {
        IflytekConfig {
            app_id: "app".into(),
            api_key: "key".into(),
            api_secret: "secret".into(),
            voice: "x3_catherine".into(),
        }
    }

    #[tokio::test]
    async fn cache_hit_returns_ok_and_does_not_hit_network() {
        let tmp = tempfile::tempdir().unwrap();
        let speaker = IflytekSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        // Pre-populate the cache for the text we'll request.
        let text = "hello cache";
        let path = speaker.cache_path_for(text);
        wav::write_wav_atomic(&path, &[0, 1, 2, 3, 4]).unwrap();

        let res = speaker.speak(text).await;
        assert!(res.is_ok(), "{res:?}");
    }

    #[tokio::test]
    async fn cache_miss_with_no_ws_yet_returns_network_error() {
        let tmp = tempfile::tempdir().unwrap();
        let speaker = IflytekSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        let err = speaker.speak("never cached").await.unwrap_err();
        assert!(matches!(err, TtsError::Network(_)));
    }

    #[test]
    fn cancel_without_active_speak_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let speaker = IflytekSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        speaker.cancel(); // must not panic
    }
}
```

- [ ] **Step 3.3: Run tests + rustfmt**

```bash
cargo test --lib tts::iflytek
rustfmt --edition 2021 --check src/tts/mod.rs src/tts/iflytek.rs
```

Expected: 3 tests pass; rustfmt silent.

- [ ] **Step 3.4: Commit**

```bash
git add src/tts/mod.rs src/tts/iflytek.rs
git commit -m "feat(tts): add IflytekSpeaker scaffold with cache-hit path and cancel plumbing"
```

---

## Task 4: WS miss path — connect, send, collect, cache-write

**Files:** `src/tts/iflytek.rs`

- [ ] **Step 4.1: Implement the WS miss branch**

Replace the `Err(TtsError::Network("WS path not implemented yet (Task 4)"...` stub in `speak` with a real WS streaming implementation. Add a private method `fn stream_ws(&self, text: &str, token: CancellationToken) -> Result<Vec<i16>, TtsError>` (actually async — `async fn`) and call it from `speak`.

In `src/tts/iflytek.rs`, add imports at the top (under the existing `use` block):

```rust
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
```

Add the streaming helper as a new `impl IflytekSpeaker` method, after `play_pcm`:

```rust
    async fn stream_ws(
        &self,
        text: &str,
        cancel: CancellationToken,
    ) -> Result<Vec<i16>, TtsError> {
        use crate::tts::frame::{build_request_frame, decode_pcm, parse_response};

        let url = self.authorized_url(SystemTime::now());
        let (ws, _resp) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| TtsError::Network(format!("ws connect: {e}")))?;
        let (mut write, mut read) = ws.split();

        let req = build_request_frame(&self.cfg.app_id, &self.cfg.voice, text);
        write
            .send(Message::Text(req))
            .await
            .map_err(|e| TtsError::Network(format!("ws send: {e}")))?;

        let mut samples = Vec::<i16>::new();
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    let _ = write.send(Message::Close(None)).await;
                    return Err(TtsError::Cancelled);
                }
                msg = read.next() => {
                    let Some(msg) = msg else {
                        return Err(TtsError::Network("ws closed before final frame".into()));
                    };
                    let msg = msg.map_err(|e| TtsError::Network(format!("ws recv: {e}")))?;
                    let text = match msg {
                        Message::Text(t) => t,
                        Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                        Message::Close(_) => {
                            return Err(TtsError::Network("ws closed by peer".into()));
                        }
                    };
                    let frame = parse_response(&text)
                        .map_err(|e| TtsError::Network(format!("frame parse: {e}")))?;
                    if !frame.is_ok() {
                        return Err(classify_iflytek_code(frame.code, &frame.message));
                    }
                    if let Some(data) = &frame.data {
                        let chunk = decode_pcm(&data.audio)
                            .map_err(|e| TtsError::Network(format!("pcm decode: {e}")))?;
                        samples.extend(chunk);
                    }
                    if frame.is_final() {
                        break;
                    }
                }
            }
        }
        Ok(samples)
    }
```

Below the `impl IflytekSpeaker` block, add a free helper:

```rust
/// Map iFlytek error codes onto `TtsError`. Any 4xx-style auth code → Auth;
/// everything else (timeouts, quota, unknown) → Network.
fn classify_iflytek_code(code: i32, message: &str) -> TtsError {
    // iFlytek auth errors cluster around 10105 / 10106 / 10107 / 11200 (quota).
    // Treat any quota-ish or auth-ish as Auth; rest as Network.
    let auth_like = matches!(code, 10105 | 10106 | 10107 | 10110 | 11200..=11210);
    let msg = format!("iflytek {code}: {message}");
    if auth_like {
        TtsError::Auth(msg)
    } else {
        TtsError::Network(msg)
    }
}
```

Now replace the `speak` body to call `stream_ws` on cache miss, write the WAV cache on success, and queue playback:

```rust
    async fn speak(&self, text: &str) -> Result<(), TtsError> {
        let path = self.cache_path_for(text);
        if path.exists() {
            let samples = wav::read_wav_pcm(&path)
                .map_err(|e| TtsError::Cache(format!("cache read: {e}")))?;
            return self.play_pcm(samples);
        }

        // Register a fresh cancel token before we do any WS work, so `cancel`
        // can interrupt us mid-stream.
        let token = CancellationToken::new();
        if let Ok(mut guard) = self.stream_handle.lock() {
            if let Some(prev) = guard.replace(token.clone()) {
                prev.cancel();
            }
        }

        let samples = self.stream_ws(text, token).await?;
        // Clear the handle on clean completion. Cancellation from outside sets
        // it to None already via `cancel`, so this is the success case only.
        if let Ok(mut guard) = self.stream_handle.lock() {
            *guard = None;
        }

        // Only cache full recordings — partial streams don't get written.
        if !samples.is_empty() {
            if let Err(e) = wav::write_wav_atomic(&path, &samples) {
                // Cache write failure is non-fatal: we still have the audio in
                // memory and want to play it. Surface as Cache only if we had
                // no samples to play anyway.
                eprintln!("tts cache write failed for {}: {e}", path.display());
            }
        }

        self.play_pcm(samples)
    }
```

(Delete the old `speak` body that returned `Network("not implemented yet")`.)

Also remove the `#[allow(dead_code)]` from `authorized_url` since `stream_ws` now uses it.

- [ ] **Step 4.2: Update scaffold test expectation**

In `src/tts/iflytek.rs` tests, the `cache_miss_with_no_ws_yet_returns_network_error` test no longer matches the new behavior (no WS server means connect will fail with a network error, but the specific message is different). Rename and relax it:

```rust
    #[tokio::test]
    async fn cache_miss_without_server_reachable_errors() {
        // Point at an obviously-unreachable URL so `connect_async` fails fast.
        let tmp = tempfile::tempdir().unwrap();
        let speaker = IflytekSpeaker::with_base_url(
            dummy_cfg(),
            tmp.path().to_path_buf(),
            None,
            "ws://127.0.0.1:1/".into(), // port 1 is privileged + usually closed
        );
        let err = speaker.speak("miss").await.unwrap_err();
        assert!(matches!(err, TtsError::Network(_)), "got {err:?}");
    }
```

- [ ] **Step 4.3: Run tests + rustfmt + clippy**

```bash
cargo test --lib tts::iflytek
rustfmt --edition 2021 --check src/tts/iflytek.rs
cargo clippy --all-targets -- -D warnings 2>&1 | grep "tts/iflytek" | head
```

Expected: 3 tests pass; rustfmt silent; no new clippy on iflytek.rs.

- [ ] **Step 4.4: Commit**

```bash
git add src/tts/iflytek.rs
git commit -m "feat(tts): implement iFlytek WS streaming with cancellation-aware select loop"
```

---

## Task 5: Update `build_speaker` factory

**Files:** `src/tts/speaker.rs`

- [ ] **Step 5.1: Extend `build_speaker` signature**

Modify `src/tts/speaker.rs`. Change the factory's body so it now returns `IflytekSpeaker` when creds are present, mode ≠ Off, and an audio handle is optionally supplied. Add an `audio` parameter.

Replace the existing `build_speaker` fn with:

```rust
/// Build the speaker appropriate for the given config and override.
/// Plan 6c: returns `IflytekSpeaker` when creds are present and mode ≠ Off;
/// otherwise `NullSpeaker`. Plan 6d will add the device-auto-detect path.
pub fn build_speaker(
    cfg: &IflytekConfig,
    cache_dir: PathBuf,
    mode: TtsOverride,
    audio: Option<rodio::OutputStreamHandle>,
) -> Box<dyn Speaker> {
    if mode == TtsOverride::Off || !has_creds(cfg) {
        return Box::new(NullSpeaker);
    }
    Box::new(crate::tts::iflytek::IflytekSpeaker::new(
        cfg.clone(),
        cache_dir,
        audio,
    ))
}
```

- [ ] **Step 5.2: Update existing tests in speaker.rs**

The three `build_speaker_returns_null_*` tests pass `(cfg, PathBuf, TtsOverride)`; each needs a new final `None` for `audio`:

In `src/tts/speaker.rs` tests, update each call site. E.g.:

```rust
    #[tokio::test]
    async fn build_speaker_returns_null_when_mode_off() {
        let b = build_speaker(
            &full_iflytek(),
            PathBuf::from("/tmp/x"),
            TtsOverride::Off,
            None,
        );
        assert!(b.speak("x").await.is_ok());
    }
```

Repeat for `build_speaker_returns_null_when_creds_missing` and the formerly-named `build_speaker_returns_null_when_creds_present_but_plan6b`. Rename the last one since it's no longer accurate — now it returns `IflytekSpeaker` with no audio handle, cache-only mode:

```rust
    #[tokio::test]
    async fn build_speaker_returns_iflytek_when_creds_present() {
        let tmp = tempfile::tempdir().unwrap();
        let b = build_speaker(
            &full_iflytek(),
            tmp.path().to_path_buf(),
            TtsOverride::On,
            None, // cache-only mode
        );
        // Exercise the cache-hit path with a pre-populated cache so we don't
        // actually try to reach iFlytek.
        let path = crate::tts::cache::cache_path(
            tmp.path(),
            &crate::tts::cache::cache_key("hello", "x3_catherine"),
        );
        crate::tts::wav::write_wav_atomic(&path, &[0, 0]).unwrap();
        let res = b.speak("hello").await;
        assert!(res.is_ok(), "{res:?}");
    }
```

- [ ] **Step 5.3: Run tests + rustfmt**

```bash
cargo test --lib tts::speaker
rustfmt --edition 2021 --check src/tts/speaker.rs
```

Expected: 6 tests pass; rustfmt silent.

- [ ] **Step 5.4: Full suite**

```bash
cargo test --all 2>&1 | grep "test result" | awk '{sum+=$4} END {print "total:", sum}'
```

Expected: baseline 210 + 5 frame + 3 iflytek scaffold + 0 net change in speaker (same 6 tests, shape updated) = 218.

- [ ] **Step 5.5: Commit**

```bash
git add src/tts/speaker.rs
git commit -m "feat(tts): build_speaker returns IflytekSpeaker when creds present"
```

---

## Task 6: Integration test with a mock WS server

**Files:** `tests/iflytek_speaker.rs`

- [ ] **Step 6.1: Create the integration test**

The test spins up a tokio-tungstenite listener on `127.0.0.1:<port>` (port 0 = OS picks), accepts one WS connection, reads the request frame, replies with a canned 3-frame response (status 0, 1, 2), closes. The speaker should: connect, send, receive 3 frames, write a WAV cache file.

Create `tests/iflytek_speaker.rs`:

```rust
//! Integration test: IflytekSpeaker against a local tokio-tungstenite mock.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures::{SinkExt, StreamExt};
use inkworm::config::IflytekConfig;
use inkworm::tts::speaker::Speaker;
use inkworm::tts::IflytekSpeaker;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn cfg() -> IflytekConfig {
    IflytekConfig {
        app_id: "app".into(),
        api_key: "key".into(),
        api_secret: "secret".into(),
        voice: "x3_catherine".into(),
    }
}

/// Spin up a one-shot mock WS server that sends three response frames
/// (status 0 + 1 + 2) carrying `total_samples` i16 PCM samples split across them.
/// Returns the `ws://127.0.0.1:<port>/` URL to connect to.
async fn start_mock_server(total_samples: u32) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}/");

    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();

        // Read one request frame (we ignore the body — the mock is trusting).
        let _req = ws.next().await.unwrap().unwrap();

        // Build three chunks of equal-ish length.
        let mut samples: Vec<i16> = (0..total_samples as usize)
            .map(|i| (i as i16).wrapping_mul(7))
            .collect();
        let third = samples.len() / 3;
        let chunk_c: Vec<i16> = samples.split_off(2 * third);
        let chunk_b: Vec<i16> = samples.split_off(third);
        let chunk_a: Vec<i16> = samples;

        for (i, chunk) in [chunk_a, chunk_b, chunk_c].iter().enumerate() {
            let status = match i { 0 => 0, 1 => 1, _ => 2 };
            let bytes: Vec<u8> = chunk.iter().flat_map(|s| s.to_le_bytes()).collect();
            let audio_b64 = BASE64.encode(&bytes);
            let frame = format!(
                r#"{{"code":0,"message":"success","sid":"test","data":{{"status":{status},"audio":"{audio_b64}","ced":"{i}"}}}}"#,
            );
            ws.send(Message::Text(frame)).await.unwrap();
        }
        let _ = ws.send(Message::Close(None)).await;
    });
    (url, handle)
}

#[tokio::test]
async fn speaker_streams_frames_and_writes_cache() {
    let (url, server) = start_mock_server(240).await;
    let tmp = tempfile::tempdir().unwrap();
    let speaker =
        IflytekSpeaker::with_base_url(cfg(), tmp.path().to_path_buf(), None, url);

    let res = speaker.speak("hello world").await;
    assert!(res.is_ok(), "{res:?}");
    let _ = server.await;

    // Cache file should exist — `hello world` + `x3_catherine` → path.
    let key = inkworm::tts::cache::cache_key("hello world", "x3_catherine");
    let path = inkworm::tts::cache::cache_path(tmp.path(), &key);
    assert!(path.exists(), "cache file should be written");

    // And it should decode back to our 240 synthetic samples.
    let got = inkworm::tts::wav::read_wav_pcm(&path).unwrap();
    assert_eq!(got.len(), 240);
}

#[tokio::test]
async fn second_speak_with_same_text_is_cache_hit() {
    let (url, server) = start_mock_server(120).await;
    let tmp = tempfile::tempdir().unwrap();
    let speaker = Arc::new(IflytekSpeaker::with_base_url(
        cfg(),
        tmp.path().to_path_buf(),
        None,
        url,
    ));
    speaker.speak("same text").await.unwrap();
    let _ = server.await;

    // Second call MUST be cache hit: no second server, so if it tried WS it would fail.
    speaker.speak("same text").await.unwrap();
}

#[tokio::test]
async fn cancel_during_stream_returns_cancelled_error() {
    // Custom server that sends one frame then stalls indefinitely.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}/");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let _req = ws.next().await.unwrap().unwrap();
        let frame = r#"{"code":0,"message":"success","sid":"x","data":{"status":0,"audio":"","ced":"0"}}"#;
        ws.send(Message::Text(frame.into())).await.unwrap();
        // Stall forever.
        tokio::time::sleep(Duration::from_secs(60)).await;
    });

    let tmp = tempfile::tempdir().unwrap();
    let speaker = Arc::new(IflytekSpeaker::with_base_url(
        cfg(),
        tmp.path().to_path_buf(),
        None,
        url,
    ));
    let speaker_bg = Arc::clone(&speaker);
    let task = tokio::spawn(async move { speaker_bg.speak("cancel me").await });

    // Give the speaker time to connect and receive the first frame.
    tokio::time::sleep(Duration::from_millis(150)).await;
    speaker.cancel();

    let res = tokio::time::timeout(Duration::from_secs(2), task).await.expect("task should finish within 2s after cancel");
    let res = res.expect("task panicked");
    assert!(
        matches!(res, Err(inkworm::tts::speaker::TtsError::Cancelled)),
        "{res:?}"
    );

    // Cache should NOT be written on cancel.
    let key = inkworm::tts::cache::cache_key("cancel me", "x3_catherine");
    let path = inkworm::tts::cache::cache_path(tmp.path(), &key);
    assert!(!path.exists());
}
```

- [ ] **Step 6.2: Run the integration test**

```bash
cargo test --test iflytek_speaker
```

Expected: 3 tests pass.

- [ ] **Step 6.3: Full suite + rustfmt**

```bash
cargo test --all 2>&1 | grep "test result" | awk '{sum+=$4} END {print "total:", sum}'
rustfmt --edition 2021 --check tests/iflytek_speaker.rs
```

Expected: total 221 (218 + 3 integration); rustfmt silent.

- [ ] **Step 6.4: Commit**

```bash
git add tests/iflytek_speaker.rs
git commit -m "test(tts): integration tests for IflytekSpeaker with mock WS server"
```

---

## Task 7: Doc sync + session log + PR

**Files:**
- Modify: `docs/superpowers/specs/2026-04-21-inkworm-design.md` (only if §7 diverged)
- Create: `docs/superpowers/progress/2026-04-22-plan-6c-session-log.md`

- [ ] **Step 7.1: Spec divergence check**

Re-read §7.1-7.6. Likely divergences:
- Error-code mapping table doesn't exist in spec (we invented `classify_iflytek_code`) — add a note if worth it.
- `play_pcm` uses `mem::forget(sink)` rather than a persisted sink that `cancel` could stop — note this limitation, Plan 6d will add a proper sink field.

Commit a `docs: sync ...` if any update lands; otherwise skip.

- [ ] **Step 7.2: Write session log**

Create `docs/superpowers/progress/2026-04-22-plan-6c-session-log.md`. Include:
- Commits (6 on this branch)
- Files added (frame.rs, iflytek.rs, iflytek_speaker.rs)
- Test counts (baseline 210 → final 221)
- Deviations (expected: `play_pcm` is fire-and-forget via `mem::forget`; Plan 6d will introduce a persisted sink field so `cancel` stops playback; error-code classifier is a heuristic)
- Follow-ups for Plan 6d (App integration, device detect, wizard TTS step, `/tts` status, proper Sink lifecycle, 3-failure session-disable)

- [ ] **Step 7.3: Final verification**

```bash
cd /Users/scguo/.tries/inkworm-tts-live
rustfmt --edition 2021 --check $(git diff --name-only main..HEAD | grep '\.rs$')
cargo clippy --all-targets -- -D warnings 2>&1 | grep -cE "^error:"   # ≤ pre-existing baseline
cargo test --all
git status    # must be clean
```

Expected: rustfmt silent on touched files; clippy unchanged; all 221 tests pass; no stray fmt noise.

- [ ] **Step 7.4: Commit session log**

```bash
git add docs/superpowers/progress/2026-04-22-plan-6c-session-log.md
git commit -m "docs: add session log for Plan 6c completion"
```

- [ ] **Step 7.5: Push and open PR**

```bash
git push -u origin feat/v1-tts-live
gh pr create --title "Plan 6c: IflytekSpeaker (live WS + rodio playback)" --body "$(cat <<'EOF'
## Summary
- \`src/tts/frame.rs\` — iFlytek request/response JSON frames + base64 PCM decoder (5 unit tests)
- \`src/tts/iflytek.rs\` — \`IflytekSpeaker\` with cache-hit + WS-streaming paths, cancellation via \`Arc<Mutex<Option<CancellationToken>>>\`, optional rodio playback (3 unit tests)
- \`build_speaker\` factory now returns \`IflytekSpeaker\` when creds present and mode ≠ Off; \`NullSpeaker\` otherwise
- \`tests/iflytek_speaker.rs\` — 3 integration tests against a local tokio-tungstenite mock server covering full stream, cache-hit on second call, and mid-stream cancel

## Non-Goals (deferred to Plan 6d)
- App integration (main.rs constructs speaker, Study drill advance triggers speak/cancel)
- Device detection (SwitchAudioSource / system_profiler), \`should_speak\` decision fn
- Config wizard TTS step (app_id / api_key / api_secret)
- \`/tts\` no-args status overlay
- Persisted Sink that \`cancel\` can stop mid-playback (currently fire-and-forget via mem::forget)

## Test plan
- [x] cargo test --all — 221 passing
- [x] rustfmt --check on touched files — clean
- [x] Three integration tests exercise the WS path end-to-end with a localhost mock
EOF
)"
```

---

## Self-Review Checklist

- **Spec coverage:**
  - §7.1 speak flow (cache hit, WS connect, frame collect, cache write, play) → Tasks 3 + 4 ✓
  - §7.2 cancellation via stream_handle → Tasks 3 + 4 (token stored, select! cancels WS) ✓ — mid-playback sink-stop is deferred to Plan 6d
  - §7.3 raw PCM 16kHz 16-bit mono → inherited from Plan 6b's wav.rs ✓
  - §7.4 URL signing → reused from Plan 6b's auth.rs ✓
  - §7.5 device detection — NOT covered; Plan 6d ✓
  - §7.6 error degradation (NullSpeaker on missing creds) → Task 5 factory ✓; 3-strikes-disable → Plan 6d
- **Placeholder scan:** every code block is complete; one intentional `#[allow(dead_code)]` is removed in Task 4 when the referenced fn is actually used.
- **Type consistency:**
  - `IflytekSpeaker::new(cfg, cache_dir, audio)` — Task 3 signature, called by factory in Task 5 ✓
  - `IflytekSpeaker::with_base_url(cfg, cache_dir, audio, base_url)` — Task 3, used by integration tests in Task 6 ✓
  - `build_speaker(cfg, cache_dir, mode, audio)` — Task 5 signature matches its 3 call sites in speaker.rs tests ✓
  - `Speaker::speak(&self, &str) -> Result<(), TtsError>` + `Speaker::cancel(&self)` — unchanged from Plan 6b ✓
  - `build_request_frame`, `parse_response`, `decode_pcm`, `FrameError` — Task 2, consumed in Task 4 ✓
- **Frequent commits:** 6 task commits + 1 session log + optional spec-sync.
- **`cargo fmt` trap:** every task uses `rustfmt --edition 2021 --check <files>`, never `cargo fmt --check`.

---

## Execution Handoff

**Plan complete.** Default = Subagent-Driven.
