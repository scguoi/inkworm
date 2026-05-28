# ElevenLabs TTS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the iFlytek WebSocket TTS provider with ElevenLabs REST as the project's sole synthesis backend, behind the existing `Speaker` trait.

**Architecture:** Drop a new `ElevenLabsSpeaker` next to the existing iFlytek one, switch the factory + wizard + UI + main wiring over to it, then delete iFlytek and prune its dependencies. Each task leaves the workspace compiling and tests green.

**Tech Stack:** Rust, tokio, reqwest (already a dep), rodio + symphonia (already used for bundle audio), wiremock (already a dev-dep).

**Spec:** `docs/superpowers/specs/2026-05-28-elevenlabs-tts-design.md`

---

## Task 1: Expose `decode_to_pcm` for the new speaker

The MP3 decoder used by `BundlePlayer` is private. ElevenLabs cache playback needs it; just bump visibility — no behavior change.

**Files:**
- Modify: `src/audio/player.rs`

- [ ] **Step 1: Bump visibility of `DecodedPcm` and `decode_to_pcm`**

In `src/audio/player.rs`, change:

```rust
struct DecodedPcm {
    samples: Vec<i16>,
    sample_rate: u32,
    channels: u16,
}

fn decode_to_pcm(path: &Path) -> Result<DecodedPcm, BundleError> {
```

to:

```rust
pub(crate) struct DecodedPcm {
    pub(crate) samples: Vec<i16>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
}

pub(crate) fn decode_to_pcm(path: &Path) -> Result<DecodedPcm, BundleError> {
```

- [ ] **Step 2: Compile and run the existing tests**

Run: `cargo test --lib`
Expected: PASS, no warnings about unused `pub(crate)`.

- [ ] **Step 3: Commit**

```bash
git add src/audio/player.rs
git commit -m "refactor(audio): expose decode_to_pcm for crate-internal reuse"
```

---

## Task 2: Add `ElevenLabsConfig` alongside the existing iFlytek config

Add the new config type and slot it into `TtsConfig` next to `iflytek`. iFlytek stays in place — this commit must still compile and pass every existing test.

**Files:**
- Modify: `src/config/defaults.rs`
- Modify: `src/config/mod.rs`
- Test: `tests/config.rs`

- [ ] **Step 1: Add ElevenLabs defaults**

In `src/config/defaults.rs`, append:

```rust
pub const DEFAULT_ELEVENLABS_VOICE_ID: &str = "21m00Tcm4TlvDq8ikWAM"; // Rachel
pub const DEFAULT_ELEVENLABS_MODEL: &str = "eleven_turbo_v2_5";
```

- [ ] **Step 2: Add `ElevenLabsConfig` struct**

In `src/config/mod.rs`, after the `IflytekConfig` block (right before `DataConfig`):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElevenLabsConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_elevenlabs_voice_id")]
    pub voice_id: String,
    #[serde(default = "default_elevenlabs_model")]
    pub model: String,
}

fn default_elevenlabs_voice_id() -> String {
    defaults::DEFAULT_ELEVENLABS_VOICE_ID.into()
}

fn default_elevenlabs_model() -> String {
    defaults::DEFAULT_ELEVENLABS_MODEL.into()
}

impl Default for ElevenLabsConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            voice_id: default_elevenlabs_voice_id(),
            model: default_elevenlabs_model(),
        }
    }
}
```

- [ ] **Step 3: Wire `ElevenLabsConfig` into `TtsConfig`**

In `src/config/mod.rs`, locate `TtsConfig` and add the new field:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TtsConfig {
    #[serde(default = "default_tts_enabled")]
    pub enabled: bool,
    #[serde(default = "default_tts_override")]
    pub r#override: TtsOverride,
    #[serde(default)]
    pub iflytek: IflytekConfig,
    #[serde(default)]
    pub elevenlabs: ElevenLabsConfig,
}
```

And in `impl Default for TtsConfig`:

```rust
impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: default_tts_enabled(),
            r#override: default_tts_override(),
            iflytek: IflytekConfig::default(),
            elevenlabs: ElevenLabsConfig::default(),
        }
    }
}
```

- [ ] **Step 4: Write a failing test for the defaults**

In `tests/config.rs`, add:

```rust
#[test]
fn elevenlabs_defaults_carry_rachel_and_turbo() {
    use inkworm::config::ElevenLabsConfig;
    let cfg = ElevenLabsConfig::default();
    assert_eq!(cfg.voice_id, "21m00Tcm4TlvDq8ikWAM");
    assert_eq!(cfg.model, "eleven_turbo_v2_5");
    assert!(cfg.api_key.is_empty());
}
```

- [ ] **Step 5: Run the test**

Run: `cargo test --test config elevenlabs_defaults_carry_rachel_and_turbo`
Expected: PASS.

- [ ] **Step 6: Run the full existing test suite**

Run: `cargo test`
Expected: every existing test still passes (we only added fields, didn't touch behavior).

- [ ] **Step 7: Commit**

```bash
git add src/config/defaults.rs src/config/mod.rs tests/config.rs
git commit -m "feat(config): add ElevenLabsConfig with Rachel + turbo defaults"
```

---

## Task 3: Scaffold `ElevenLabsSpeaker` (cache-hit + cancel only)

Drop in the new speaker module. The cache-miss branch is a placeholder for now; Task 4 fills it in. Not yet wired to the factory.

**Files:**
- Create: `src/tts/elevenlabs.rs`
- Modify: `src/tts/mod.rs` (declare module)

- [ ] **Step 1: Declare the module**

In `src/tts/mod.rs`, after the existing module declarations (look for `pub mod iflytek;` or similar in the top of the file — currently there is no explicit `mod iflytek;` because each file is implicitly a module under `tts/`; we need to make sure `mod elevenlabs;` appears). Inspect first:

```bash
grep -n "^pub mod\|^mod " src/tts/mod.rs
```

Then add `pub mod elevenlabs;` next to the other `pub mod` declarations.

- [ ] **Step 2: Create the speaker scaffold**

Create `src/tts/elevenlabs.rs` with:

```rust
//! ElevenLabs REST TTS speaker — POST /v1/text-to-speech + MP3 cache + rodio playback.
//!
//! Cache miss is filled in by Task 4; this module ships the scaffold,
//! cache-hit playback, and cancellation plumbing.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::audio::player::{decode_to_pcm, DecodedPcm};
use crate::config::ElevenLabsConfig;
use crate::tts::speaker::{Speaker, TtsError};

const DEFAULT_BASE_URL: &str = "https://api.elevenlabs.io";

pub struct ElevenLabsSpeaker {
    cfg: ElevenLabsConfig,
    cache_dir: PathBuf,
    base_url: String,
    http: reqwest::Client,
    audio: Option<rodio::OutputStreamHandle>,
    current_sink: Arc<Mutex<Option<rodio::Sink>>>,
    generation: Arc<AtomicU64>,
}

impl ElevenLabsSpeaker {
    pub fn new(
        cfg: ElevenLabsConfig,
        cache_dir: PathBuf,
        audio: Option<rodio::OutputStreamHandle>,
    ) -> Self {
        Self::with_base_url(cfg, cache_dir, DEFAULT_BASE_URL.to_string(), audio)
    }

    pub fn with_base_url(
        cfg: ElevenLabsConfig,
        cache_dir: PathBuf,
        base_url: String,
        audio: Option<rodio::OutputStreamHandle>,
    ) -> Self {
        Self {
            cfg,
            cache_dir,
            base_url,
            http: reqwest::Client::new(),
            audio,
            current_sink: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    fn cache_path_for(&self, text: &str) -> PathBuf {
        let mut hasher = blake3::Hasher::new();
        hasher.update(text.as_bytes());
        hasher.update(b"\n");
        hasher.update(self.cfg.voice_id.as_bytes());
        hasher.update(b"\n");
        hasher.update(self.cfg.model.as_bytes());
        let key = hasher.finalize().to_hex().to_string();
        self.cache_dir.join(format!("{key}.mp3"))
    }

    /// Decode the cached MP3 at `path` and install a fresh `rodio::Sink`
    /// containing its PCM. Re-checks `generation` before installing so a
    /// cancel that arrived during the decode short-circuits the install.
    fn play_cached(&self, path: &std::path::Path, started_gen: u64) -> Result<(), TtsError> {
        if self.generation.load(Ordering::SeqCst) != started_gen {
            return Ok(()); // cancelled mid-cache-read
        }
        let Some(audio) = self.audio.as_ref() else {
            return Ok(()); // cache-only mode (headless / tests)
        };
        let DecodedPcm {
            samples,
            sample_rate,
            channels,
        } = decode_to_pcm(path).map_err(|e| TtsError::Audio(format!("decode: {e}")))?;
        if self.generation.load(Ordering::SeqCst) != started_gen {
            return Ok(());
        }
        let sink = rodio::Sink::try_new(audio).map_err(|e| TtsError::Audio(e.to_string()))?;
        sink.append(rodio::buffer::SamplesBuffer::new(
            channels, sample_rate, samples,
        ));
        if let Ok(mut guard) = self.current_sink.lock() {
            if self.generation.load(Ordering::SeqCst) != started_gen {
                drop(sink);
                return Ok(());
            }
            if let Some(old) = guard.take() {
                old.stop();
            }
            *guard = Some(sink);
        }
        Ok(())
    }
}

#[async_trait]
impl Speaker for ElevenLabsSpeaker {
    async fn speak(&self, text: &str) -> Result<(), TtsError> {
        let started_gen = self.generation.load(Ordering::SeqCst);
        let path = self.cache_path_for(text);
        if path.exists() {
            tracing::info!(
                text_len = text.len(),
                cache_hit = true,
                "ElevenLabs cache hit"
            );
            return self.play_cached(&path, started_gen);
        }
        // Task 4 fills this in.
        Err(TtsError::Network(
            "elevenlabs cache-miss not yet implemented".into(),
        ))
    }

    fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut guard) = self.current_sink.lock() {
            if let Some(sink) = guard.take() {
                sink.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_cfg() -> ElevenLabsConfig {
        ElevenLabsConfig {
            api_key: "sk_test".into(),
            voice_id: "voice_test".into(),
            model: "model_test".into(),
        }
    }

    #[test]
    fn cache_path_changes_with_text() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ElevenLabsSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        assert_ne!(s.cache_path_for("a"), s.cache_path_for("b"));
    }

    #[test]
    fn cache_path_changes_with_voice_id() {
        let tmp = tempfile::tempdir().unwrap();
        let s1 = ElevenLabsSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        let mut cfg2 = dummy_cfg();
        cfg2.voice_id = "different".into();
        let s2 = ElevenLabsSpeaker::new(cfg2, tmp.path().to_path_buf(), None);
        assert_ne!(s1.cache_path_for("x"), s2.cache_path_for("x"));
    }

    #[test]
    fn cache_path_changes_with_model() {
        let tmp = tempfile::tempdir().unwrap();
        let s1 = ElevenLabsSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        let mut cfg2 = dummy_cfg();
        cfg2.model = "different".into();
        let s2 = ElevenLabsSpeaker::new(cfg2, tmp.path().to_path_buf(), None);
        assert_ne!(s1.cache_path_for("x"), s2.cache_path_for("x"));
    }

    #[test]
    fn cache_path_uses_mp3_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ElevenLabsSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        let p = s.cache_path_for("hello");
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("mp3"));
    }

    #[tokio::test]
    async fn cache_hit_returns_ok_without_network() {
        // Write an empty file at the cache path; in cache-only mode (audio=None)
        // play_cached short-circuits before touching it, so this passes without
        // a real MP3.
        let tmp = tempfile::tempdir().unwrap();
        let s = ElevenLabsSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        let p = s.cache_path_for("hello");
        std::fs::write(&p, b"").unwrap();
        let res = s.speak("hello").await;
        assert!(res.is_ok(), "got {res:?}");
    }

    #[test]
    fn cancel_bumps_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ElevenLabsSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        let before = s.generation.load(Ordering::SeqCst);
        s.cancel();
        let after = s.generation.load(Ordering::SeqCst);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn cancel_without_in_flight_speak_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ElevenLabsSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        s.cancel(); // must not panic
    }
}
```

- [ ] **Step 3: Run the new tests**

Run: `cargo test --lib tts::elevenlabs`
Expected: all 7 tests pass.

- [ ] **Step 4: Make sure the rest of the suite still compiles + passes**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/tts/mod.rs src/tts/elevenlabs.rs
git commit -m "feat(tts): scaffold ElevenLabsSpeaker with cache-hit playback"
```

---

## Task 4: Implement the HTTP cache-miss branch with mocked tests

Fill in `speak`'s cache-miss path: POST to ElevenLabs, atomically write the MP3 response into the cache, then fall through to `play_cached`. Use `wiremock` for tests.

**Files:**
- Modify: `src/tts/elevenlabs.rs`

- [ ] **Step 1: Write a failing test for the cache-miss success path**

Add to `src/tts/elevenlabs.rs` `#[cfg(test)] mod tests`:

```rust
#[tokio::test]
async fn cache_miss_posts_to_api_and_writes_cache() {
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/text-to-speech/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"fake-mp3-bytes".to_vec()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let s = ElevenLabsSpeaker::with_base_url(
        dummy_cfg(),
        tmp.path().to_path_buf(),
        server.uri(),
        None, // cache-only mode: no decode attempted
    );

    let res = s.speak("hello").await;
    assert!(res.is_ok(), "got {res:?}");

    let cached = s.cache_path_for("hello");
    assert!(cached.exists(), "cache file must be written");
    let bytes = std::fs::read(&cached).unwrap();
    assert_eq!(bytes, b"fake-mp3-bytes");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --lib tts::elevenlabs::tests::cache_miss_posts_to_api_and_writes_cache`
Expected: FAIL with `TtsError::Network("...not yet implemented...")`.

- [ ] **Step 3: Implement the cache-miss branch**

In `src/tts/elevenlabs.rs`, replace the `speak` method's body:

```rust
async fn speak(&self, text: &str) -> Result<(), TtsError> {
    let started_gen = self.generation.load(Ordering::SeqCst);
    let path = self.cache_path_for(text);
    if path.exists() {
        tracing::info!(
            text_len = text.len(),
            cache_hit = true,
            "ElevenLabs cache hit"
        );
        return self.play_cached(&path, started_gen);
    }

    let url = format!(
        "{}/v1/text-to-speech/{}",
        self.base_url.trim_end_matches('/'),
        self.cfg.voice_id
    );
    let body = serde_json::json!({
        "text": text,
        "model_id": self.cfg.model,
    });

    let resp = self
        .http
        .post(&url)
        .header("xi-api-key", &self.cfg.api_key)
        .header("Accept", "audio/mpeg")
        .json(&body)
        .send()
        .await
        .map_err(|e| TtsError::Network(format!("send: {e}")))?;

    let status = resp.status();
    if status.is_success() {
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| TtsError::Network(format!("body: {e}")))?;

        if self.generation.load(Ordering::SeqCst) != started_gen {
            return Ok(()); // cancelled while we were downloading
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| TtsError::Cache(format!("mkdir: {e}")))?;
        }
        crate::storage::atomic::write_atomic(&path, &bytes)
            .map_err(|e| TtsError::Cache(format!("write: {e}")))?;

        tracing::info!(
            text_len = text.len(),
            cache_hit = false,
            bytes = bytes.len(),
            "ElevenLabs synthesis ok"
        );
        return self.play_cached(&path, started_gen);
    }

    let body_text = resp.text().await.unwrap_or_default();
    let msg = format!("elevenlabs {status}: {body_text}");
    if status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
    {
        Err(TtsError::Auth(msg))
    } else {
        Err(TtsError::Network(msg))
    }
}
```

- [ ] **Step 4: Run the new test**

Run: `cargo test --lib tts::elevenlabs::tests::cache_miss_posts_to_api_and_writes_cache`
Expected: PASS.

- [ ] **Step 5: Add the auth-failure test**

```rust
#[tokio::test]
async fn http_401_maps_to_auth_error() {
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/text-to-speech/.*"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let s = ElevenLabsSpeaker::with_base_url(
        dummy_cfg(),
        tmp.path().to_path_buf(),
        server.uri(),
        None,
    );
    let err = s.speak("hello").await.unwrap_err();
    assert!(matches!(err, TtsError::Auth(_)), "got {err:?}");
}
```

- [ ] **Step 6: Run it**

Run: `cargo test --lib tts::elevenlabs::tests::http_401_maps_to_auth_error`
Expected: PASS.

- [ ] **Step 7: Add the 429 → Network test**

```rust
#[tokio::test]
async fn http_429_maps_to_network_error() {
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/text-to-speech/.*"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate-limited"))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let s = ElevenLabsSpeaker::with_base_url(
        dummy_cfg(),
        tmp.path().to_path_buf(),
        server.uri(),
        None,
    );
    let err = s.speak("hello").await.unwrap_err();
    assert!(matches!(err, TtsError::Network(_)), "got {err:?}");
}
```

Run: `cargo test --lib tts::elevenlabs::tests::http_429_maps_to_network_error`
Expected: PASS.

- [ ] **Step 8: Add the unreachable-host test**

```rust
#[tokio::test]
async fn unreachable_host_maps_to_network_error() {
    let tmp = tempfile::tempdir().unwrap();
    let s = ElevenLabsSpeaker::with_base_url(
        dummy_cfg(),
        tmp.path().to_path_buf(),
        "http://127.0.0.1:1".into(), // reserved port, refused
        None,
    );
    let err = s.speak("hello").await.unwrap_err();
    assert!(matches!(err, TtsError::Network(_)), "got {err:?}");
}
```

Run: `cargo test --lib tts::elevenlabs::tests::unreachable_host_maps_to_network_error`
Expected: PASS.

- [ ] **Step 9: Run the whole suite to verify nothing else regressed**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add src/tts/elevenlabs.rs
git commit -m "feat(tts): implement ElevenLabs HTTP cache-miss branch"
```

---

## Task 5: Switch `build_speaker`, `main.rs`, and `app.rs` to ElevenLabs

Flip the factory and call sites. After this commit, **iFlytek code is dead** (unused functions warned) but kept in the tree — Task 9 deletes it. The runtime now uses ElevenLabs.

**Files:**
- Modify: `src/tts/speaker.rs`
- Modify: `src/main.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Update `build_speaker` to consume `ElevenLabsConfig`**

In `src/tts/speaker.rs`, replace the `build_speaker` function and its imports:

```rust
use crate::config::{ElevenLabsConfig, TtsOverride};

// ...

/// Build the speaker appropriate for the given config and override.
/// Returns `ElevenLabsSpeaker` when the API key is set and mode ≠ Off;
/// otherwise `NullSpeaker`.
pub fn build_speaker(
    cfg: &ElevenLabsConfig,
    cache_dir: PathBuf,
    mode: TtsOverride,
    audio: Option<rodio::OutputStreamHandle>,
) -> Box<dyn Speaker> {
    if mode == TtsOverride::Off || cfg.api_key.trim().is_empty() {
        return Box::new(NullSpeaker);
    }
    Box::new(crate::tts::elevenlabs::ElevenLabsSpeaker::new(
        cfg.clone(),
        cache_dir,
        audio,
    ))
}

fn has_creds(cfg: &ElevenLabsConfig) -> bool {
    !cfg.api_key.trim().is_empty()
}
```

Delete the old `IflytekConfig`-based body and the `use crate::config::IflytekConfig;` import.

- [ ] **Step 2: Update the tests in `src/tts/speaker.rs` to use ElevenLabs**

In the `#[cfg(test)] mod tests` block at the bottom of the same file, replace `IflytekConfig` helpers with ElevenLabs equivalents:

```rust
fn empty_elevenlabs() -> ElevenLabsConfig {
    ElevenLabsConfig {
        api_key: String::new(),
        voice_id: "v".into(),
        model: "m".into(),
    }
}

fn full_elevenlabs() -> ElevenLabsConfig {
    ElevenLabsConfig {
        api_key: "sk_test".into(),
        voice_id: "v".into(),
        model: "m".into(),
    }
}
```

Then update the existing tests in that mod that referenced `IflytekConfig` to call these helpers, and adjust the `has_creds_requires_all_three_nonempty` test to a single-field check:

```rust
#[test]
fn has_creds_requires_api_key() {
    assert!(!has_creds(&empty_elevenlabs()));
    assert!(has_creds(&full_elevenlabs()));
}
```

Remove any test that explicitly tested the three-field requirement.

- [ ] **Step 3: Update `src/main.rs` to pass `tts.elevenlabs`**

In `src/main.rs` find the `build_speaker(...)` call (around line 178):

```rust
let speaker: Arc<dyn inkworm::tts::speaker::Speaker> = Arc::from(build_speaker(
    &config.tts.iflytek,
    paths.tts_cache_dir.clone(),
    config.tts.r#override,
    audio_handle.clone(),
));
```

Change `&config.tts.iflytek` to `&config.tts.elevenlabs`.

- [ ] **Step 4: Update `src/app.rs::tts_has_creds`**

Around line 504-509:

```rust
fn tts_has_creds(&self) -> bool {
    let cfg = &self.config.tts.iflytek;
    !cfg.app_id.trim().is_empty()
        && !cfg.api_key.trim().is_empty()
        && !cfg.api_secret.trim().is_empty()
}
```

Replace with:

```rust
fn tts_has_creds(&self) -> bool {
    !self.config.tts.elevenlabs.api_key.trim().is_empty()
}
```

- [ ] **Step 5: Update `src/app.rs`'s wizard-probe site**

Around line 1799:

```rust
let iflytek = match self.config_wizard.as_ref() {
    Some(s) => s.draft.tts.iflytek.clone(),
    None => return,
};
```

Change `iflytek` (variable + field) to `elevenlabs`:

```rust
let elevenlabs = match self.config_wizard.as_ref() {
    Some(s) => s.draft.tts.elevenlabs.clone(),
    None => return,
};
```

Then the next few lines that pass `iflytek` into `probe_tts` will need updating once Task 8 changes `probe_tts`'s signature — but for now we don't break the call: `probe_tts` still takes `IflytekConfig`. Leave the variable named `elevenlabs` but pass `self.config_wizard.as_ref().unwrap().draft.tts.iflytek.clone()` into `probe_tts` for now to keep things compiling.

Wait — re-think. The cleanest fix: do **both** in one task. So in this same task we also change `probe_tts` to take `ElevenLabsConfig`. Update `src/ui/config_wizard.rs::probe_tts`:

```rust
pub async fn probe_tts(
    elevenlabs: crate::config::ElevenLabsConfig,
    cancel: CancellationToken,
) -> Result<(), AppError> {
    use crate::config::TtsOverride;
    use crate::tts::speaker::build_speaker;

    let cache_dir = std::env::temp_dir().join("inkworm-tts-probe");
    std::fs::create_dir_all(&cache_dir).ok();

    let speaker = build_speaker(&elevenlabs, cache_dir, TtsOverride::On, None);

    tokio::select! {
        res = speaker.speak("hello") => {
            res.map_err(AppError::Tts)
        }
        _ = cancel.cancelled() => Err(AppError::Cancelled),
    }
}
```

Then back in `src/app.rs` the call site becomes:

```rust
let elevenlabs = match self.config_wizard.as_ref() {
    Some(s) => s.draft.tts.elevenlabs.clone(),
    None => return,
};

// ... existing setup ...
let msg = match probe_tts(elevenlabs, cancel).await {
    // ...
};
```

- [ ] **Step 6: Run the whole suite**

Run: `cargo test`
Expected: PASS. Compiler will warn that the iFlytek module is now unused — that's OK and expected; Task 9 deletes it.

- [ ] **Step 7: Commit**

```bash
git add src/tts/speaker.rs src/main.rs src/app.rs src/ui/config_wizard.rs
git commit -m "feat(tts): wire build_speaker, app, and probe to ElevenLabs"
```

---

## Task 6: Update the wizard's step flow

Drop `TtsAppId` and `TtsApiSecret`; rebind `TtsApiKey` to `tts.elevenlabs.api_key`. Total steps drop from 7 to 5.

**Files:**
- Modify: `src/ui/config_wizard.rs`

- [ ] **Step 1: Update the `WizardStep` enum**

In `src/ui/config_wizard.rs` (around line 20), remove `TtsAppId` and `TtsApiSecret`:

```rust
pub enum WizardStep {
    Endpoint,
    ApiKey,
    Model,
    TtsEnable,
    TtsApiKey,
}
```

- [ ] **Step 2: Renumber `step_number` and update `total_steps`**

Replace the body of `step_number` (around line 103) so the remaining variants get 1..=5:

```rust
pub fn step_number(&self) -> usize {
    match self.step {
        WizardStep::Endpoint => 1,
        WizardStep::ApiKey => 2,
        WizardStep::Model => 3,
        WizardStep::TtsEnable => 4,
        WizardStep::TtsApiKey => 5,
    }
}

pub fn total_steps(&self) -> usize {
    5
}
```

If `total_steps` was hard-coded to `7` elsewhere in this file, fix it now (grep `7` near `total_steps`).

- [ ] **Step 3: Update the empty-input check**

Around line 127:

```rust
if trimmed.is_empty() && self.step != WizardStep::TtsEnable {
    let reason = match self.step {
        WizardStep::Endpoint => "Endpoint cannot be empty",
        WizardStep::ApiKey => "API key cannot be empty",
        WizardStep::Model => "Model cannot be empty",
        WizardStep::TtsApiKey => "API key cannot be empty",
        WizardStep::TtsEnable => unreachable!(),
    };
    // ...existing error-return path...
}
```

(Drop the three iFlytek branches.)

- [ ] **Step 4: Update the advance logic**

Around lines 145–195 the `match self.step { ... }` block branches per step. The new flow:

```rust
match self.step {
    WizardStep::Endpoint => {
        self.draft.llm.base_url = trimmed.to_string();
        self.step = WizardStep::ApiKey;
        self.input = self.draft.llm.api_key.clone();
    }
    WizardStep::ApiKey => {
        self.draft.llm.api_key = trimmed.to_string();
        self.step = WizardStep::Model;
        self.input = self.draft.llm.model.clone();
    }
    WizardStep::Model => {
        self.draft.llm.model = trimmed.to_string();
        self.step = WizardStep::TtsEnable;
        // (existing "y/n" prompt setup — leave alone)
    }
    WizardStep::TtsEnable => {
        // y branch:
        if /* user chose yes */ {
            self.step = WizardStep::TtsApiKey;
            self.input = self.draft.tts.elevenlabs.api_key.clone();
        }
        // n branch unchanged (likely sets tts.enabled = false and finishes)
    }
    WizardStep::TtsApiKey => {
        self.draft.tts.elevenlabs.api_key = trimmed.to_string();
        // Fire the connectivity probe (existing pattern that was on TtsApiSecret).
        self.tts_probe_requested = true;
        // Whatever finalization the wizard does next stays the same.
    }
}
```

Read the existing TtsEnable / TtsApiSecret branches carefully and port the connectivity-probe trigger from TtsApiSecret to TtsApiKey. The probe state field name is likely `tts_probe_requested` — keep whatever the existing code uses.

- [ ] **Step 5: Update the back-navigation block**

Around lines 215–250 there's logic that re-pulls `self.input` when stepping back. Drop any case that pulls from `tts.iflytek.app_id` / `api_secret`. Keep:

```rust
WizardStep::TtsApiKey => {
    self.input = self.draft.tts.elevenlabs.api_key.clone();
}
```

- [ ] **Step 6: Update existing wizard unit tests in `src/ui/config_wizard.rs`**

Find tests around line 563+ that reference `tts.iflytek.app_id`, `tts.iflytek.api_key`, `tts.iflytek.api_secret`, or expected step transitions like `TtsEnable → TtsAppId`. Rewrite to the new flow:

```rust
#[test]
fn tts_enable_y_advances_to_tts_api_key() {
    let mut w = wizard_after_llm_done();
    // Simulate user saying 'y' on TtsEnable. (Use whatever helper exists.)
    advance_with_y(&mut w);
    assert_eq!(w.step, WizardStep::TtsApiKey);
}

#[test]
fn tts_api_key_step_writes_into_elevenlabs() {
    let mut w = wizard_at(WizardStep::TtsApiKey);
    w.input = "sk_realkey".into();
    w.try_advance().unwrap(); // or whatever the existing advance method is
    assert_eq!(w.draft.tts.elevenlabs.api_key, "sk_realkey");
}
```

Delete any tests that asserted `tts.iflytek.app_id` / `api_secret` were set.

- [ ] **Step 7: Run wizard tests**

Run: `cargo test --lib ui::config_wizard`
Expected: PASS.

- [ ] **Step 8: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/ui/config_wizard.rs
git commit -m "feat(wizard): collapse iFlytek 3-step setup into single ElevenLabs key step"
```

---

## Task 7: Update `doctor.rs` and `tts_status.rs` to use ElevenLabs

Both UIs still inspect `config.tts.iflytek.*` for credential presence. Switch to ElevenLabs.

**Files:**
- Modify: `src/ui/doctor.rs`
- Modify: `src/ui/tts_status.rs`

- [ ] **Step 1: Update `doctor.rs`**

Around line 96:

```rust
let creds_ok = !config.tts.iflytek.app_id.trim().is_empty()
    && !config.tts.iflytek.api_key.trim().is_empty()
    && !config.tts.iflytek.api_secret.trim().is_empty();
```

Replace with:

```rust
let creds_ok = !config.tts.elevenlabs.api_key.trim().is_empty();
```

- [ ] **Step 2: Update `tts_status.rs`**

Around line 34:

```rust
let creds_ok = !config.iflytek.app_id.trim().is_empty()
    && !config.iflytek.api_key.trim().is_empty()
    && !config.iflytek.api_secret.trim().is_empty();
```

Replace with:

```rust
let creds_ok = !config.elevenlabs.api_key.trim().is_empty();
```

- [ ] **Step 3: Fix the test fixture in `tts_status.rs`**

Around line 122 there's a test fixture that constructs `IflytekConfig`. Replace with `ElevenLabsConfig`:

```rust
use crate::config::{ElevenLabsConfig, TtsConfig, TtsOverride};
// ...
TtsConfig {
    enabled: true,
    r#override: TtsOverride::Auto,
    iflytek: Default::default(),         // leave iflytek default for now
    elevenlabs: ElevenLabsConfig {
        api_key: "sk_test".into(),
        voice_id: "v".into(),
        model: "m".into(),
    },
}
```

- [ ] **Step 4: Update `tests/config_wizard.rs`**

The integration test around line 151 also patches `tts.iflytek`. Replace with `tts.elevenlabs`:

```rust
existing.tts.elevenlabs = inkworm::config::ElevenLabsConfig {
    api_key: "sk_test".into(),
    voice_id: "v".into(),
    model: "m".into(),
};
```

Remove the `IflytekConfig` import if no longer used.

- [ ] **Step 5: Run the suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/doctor.rs src/ui/tts_status.rs tests/config_wizard.rs
git commit -m "refactor(ui): swap iFlytek creds check for ElevenLabs in doctor and status"
```

---

## Task 8: Switch `validate_tts` to ElevenLabs

The validator still flags the three iFlytek fields. Switch it to a single check on the ElevenLabs API key.

**Files:**
- Modify: `src/config/mod.rs`
- Modify: `tests/config.rs`

- [ ] **Step 1: Replace the validation body**

In `src/config/mod.rs::validate_tts` (around line 250):

```rust
pub fn validate_tts(&self) -> Vec<ConfigError> {
    let mut errs = Vec::new();
    if self.tts.enabled && self.tts.r#override != TtsOverride::Off {
        if self.tts.elevenlabs.api_key.trim().is_empty() {
            errs.push(ConfigError::MissingField("tts.elevenlabs.api_key"));
        }
    }
    errs
}
```

- [ ] **Step 2: Update the tests in `tests/config.rs`**

Around lines 20–80 there are tests that set `c.tts.iflytek.app_id = "a"` etc. Convert them to set `c.tts.elevenlabs.api_key = "sk"`:

```rust
let mut c = Config::default();
c.tts.elevenlabs.api_key = "sk_test".into();
assert!(c.validate_tts().is_empty());
```

And the negative test that expected "MissingField tts.iflytek.app_id" should now expect `"tts.elevenlabs.api_key"`.

- [ ] **Step 3: Run the config tests**

Run: `cargo test --test config`
Expected: PASS.

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs tests/config.rs
git commit -m "refactor(config): validate_tts checks ElevenLabs api_key only"
```

---

## Task 9: Delete iFlytek module, types, deps

iFlytek code is now unused. Excise it.

**Files:**
- Delete: `src/tts/iflytek.rs`, `src/tts/auth.rs`, `src/tts/frame.rs`, `src/tts/wav.rs`, `src/tts/snapshots/`
- Modify: `src/tts/mod.rs`
- Modify: `src/config/mod.rs`
- Modify: `src/config/defaults.rs`
- Modify: `tests/bundled_audio.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Remove the module file declarations**

In `src/tts/mod.rs`, drop `pub mod iflytek;`, `pub mod auth;`, `pub mod frame;`, `pub mod wav;` (whichever exist).

- [ ] **Step 2: Delete the iFlytek source files**

```bash
git rm src/tts/iflytek.rs src/tts/auth.rs src/tts/frame.rs src/tts/wav.rs
git rm -r src/tts/snapshots
```

- [ ] **Step 3: Remove `IflytekConfig` and its default**

In `src/config/mod.rs`:
- Delete the `IflytekConfig` struct + `Default` impl (around lines 158–183).
- Delete the `default_voice` function (around line 169).
- Remove the `iflytek: IflytekConfig` field from `TtsConfig`.
- Remove the `iflytek: IflytekConfig::default(),` from `impl Default for TtsConfig`.

In `src/config/defaults.rs`, remove `pub const DEFAULT_IFLYTEK_VOICE: ...`.

- [ ] **Step 4: Fix any test that still references `IflytekConfig`**

Run `grep -rn "iflytek\|IflytekConfig\|DEFAULT_IFLYTEK_VOICE" src tests` and clean up each hit. In particular:

- `tests/bundled_audio.rs` around lines 63–65 sets three iFlytek fields. Replace with:

  ```rust
  config.tts.elevenlabs.api_key = "sk_test".into();
  ```

- Rename `bundled_hit_works_when_no_iflytek_creds` (line 247) to `bundled_hit_works_when_no_tts_creds` and change its body accordingly (don't set `api_key`).

- In `src/ui/tts_status.rs` remove the now-unused `iflytek: Default::default(),` field initializer from the test fixture (it would fail compilation after the struct field is gone).

- [ ] **Step 5: Drop unused dependencies**

In `Cargo.toml`, remove:

```toml
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
httpdate = "1"
```

Also check whether `blake3` is still used (it is — by `tts/elevenlabs.rs::cache_path_for` and `audio/bundle.rs`). Keep it.

- [ ] **Step 6: Build to confirm there are no orphaned references**

Run: `cargo build`
Expected: PASS, no errors.

- [ ] **Step 7: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(tts): remove iFlytek module, types, and WS deps"
```

(Use `git add -A` here because there are deletions plus modifications across many files. Verify with `git status` before commit that no unintended files are staged.)

---

## Task 10: Normalize the cache module to MP3

ElevenLabs writes `.mp3` files via its own inline path computation. Move that into `cache.rs` so the cache module is the single source of truth for naming, and update `cache_stats` + `clear_cache` to match.

**Files:**
- Modify: `src/tts/cache.rs`
- Modify: `src/tts/mod.rs`
- Modify: `src/tts/elevenlabs.rs`

- [ ] **Step 1: Update `cache::cache_key` to include the model**

In `src/tts/cache.rs`:

```rust
/// Derive a stable cache key from text, voice id, and model.
pub fn cache_key(text: &str, voice_id: &str, model: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(text.as_bytes());
    hasher.update(b"\n");
    hasher.update(voice_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(model.as_bytes());
    hasher.finalize().to_hex().to_string()
}
```

- [ ] **Step 2: Update `cache::cache_path` to `.mp3`**

```rust
pub fn cache_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.mp3"))
}
```

- [ ] **Step 3: Update `cache::cache_stats` to filter `.mp3`**

In the filter line:

```rust
if path.extension().and_then(|e| e.to_str()) != Some("mp3") {
    continue;
}
```

- [ ] **Step 4: Update the unit tests in `cache.rs`**

Every test that called `cache_key(text, voice)` now passes three args. Update the assertions for the extension check. Add a test that the model affects the key:

```rust
#[test]
fn different_model_produces_different_key() {
    let a = cache_key("hello", "v", "model_a");
    let b = cache_key("hello", "v", "model_b");
    assert_ne!(a, b);
}

#[test]
fn cache_path_uses_mp3_extension() {
    let p = cache_path(Path::new("/tmp/inkworm/tts-cache"), "abc");
    assert_eq!(p.to_str(), Some("/tmp/inkworm/tts-cache/abc.mp3"));
}
```

Update all existing tests that wrote `a.wav` to write `a.mp3` instead.

- [ ] **Step 5: Update `tts/mod.rs::clear_cache` to filter `.mp3`**

In `src/tts/mod.rs`, change the filter:

```rust
if path.extension().and_then(|e| e.to_str()) != Some("mp3") {
    continue;
}
```

And update the docstring (`.wav` → `.mp3`).

- [ ] **Step 6: Have `ElevenLabsSpeaker` use the cache module**

In `src/tts/elevenlabs.rs`, replace the inline `cache_path_for` with a call into `cache.rs`:

```rust
fn cache_path_for(&self, text: &str) -> PathBuf {
    let key = crate::tts::cache::cache_key(text, &self.cfg.voice_id, &self.cfg.model);
    crate::tts::cache::cache_path(&self.cache_dir, &key)
}
```

- [ ] **Step 7: Run the speaker tests**

Run: `cargo test --lib tts::elevenlabs`
Expected: PASS — the cache path now comes from the module but produces the same hex+`.mp3` shape.

- [ ] **Step 8: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/tts/cache.rs src/tts/mod.rs src/tts/elevenlabs.rs
git commit -m "refactor(tts): centralize MP3 cache naming in cache module"
```

---

## Task 11: Install + manual verify

Per `CLAUDE.md`, install to `~/.cargo/bin` after a user-facing change and report the version.

**Files:** none

- [ ] **Step 1: Install the new binary**

```bash
cargo install --path . --force
```

Expected: clean build, "Installed" log line.

- [ ] **Step 2: Verify the version**

```bash
inkworm --version
```

Report the printed version (should be `inkworm 0.2.30` or whatever the current `Cargo.toml` says) back to the user in the final summary.

- [ ] **Step 3: Hand off the manual test plan to the user**

Tell the user to:

1. Run `inkworm`, hit the config wizard if the existing iFlytek config triggers a load failure. (The wizard now has 5 steps; the TTS step will ask for the ElevenLabs API key.)
2. Paste the test key `sk_fa6bfe4dacd928cc28f12189a35a8b8b75367dc0b2d618e2`.
3. Pick a course and type a drill correctly; confirm audio plays from ElevenLabs (not a bundled file).
4. Type the same drill again (or restart and replay); confirm the cache hit avoids a new HTTP call (`INKWORM_LOG=debug inkworm` should show `cache_hit=true`).
5. Run `/tts clear-cache`, then type the drill again — confirm a fresh request fires.

Do not commit anything else in this task.

---

## Self-Review Notes

- **Spec coverage:** Every section of the spec maps to a task: config (Task 2, 8, 9), speaker impl (Tasks 3, 4), caching (Tasks 3, 10), factory (Task 5), wizard (Task 6), doctor + status (Task 7), deletions + deps (Task 9), tests (within each), migration / manual verify (Task 11).
- **No placeholders:** Every step has either exact code, an exact `cargo` command, or precise file-path edit instructions.
- **Type consistency:** `ElevenLabsConfig { api_key, voice_id, model }` is used identically across tasks. `cache_key` consistently takes three string arguments after Task 10. `build_speaker` consistently takes `&ElevenLabsConfig` after Task 5.
- **Iflytek-during-transition note:** Tasks 2–8 leave the dead `IflytekConfig` field in `TtsConfig` so each commit compiles; Task 9 is the one that removes it. Test fixtures in `tts_status.rs` and `tests/config_wizard.rs` reflect this — they keep `iflytek: Default::default()` until Task 9.
