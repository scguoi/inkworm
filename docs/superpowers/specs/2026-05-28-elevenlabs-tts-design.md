# ElevenLabs TTS — design

**Status:** approved, ready for implementation plan
**Date:** 2026-05-28

## Goal

Replace iFlytek WebSocket TTS with ElevenLabs REST TTS as the project's sole
synthesis provider. The `Speaker` trait stays; only one implementation lives
behind it.

## Motivation

iFlytek requires three credentials (app_id / api_key / api_secret), uses a
custom HMAC-signed WebSocket protocol, and ships a hand-rolled WAV writer.
ElevenLabs offers a one-key REST API returning MP3, which Symphonia (already
in the dependency tree for bundle audio) can decode. Removing iFlytek
eliminates `tokio-tungstenite`, `httpdate`, the WAV writer, and ~400 lines of
WS framing and auth code.

## Non-goals

- Keeping iFlytek as a fallback. The user opted for full replacement.
- Streaming synthesis. Drill sentences are short; one-shot REST + cache is
  simpler and the cache hit rate is high after a course's first pass.
- Migrating existing `tts.iflytek` config blocks. Old configs fail to load
  (`deny_unknown_fields`), the wizard re-prompts. Users have one machine.
- Switching providers at runtime. Compile-time choice.

## Configuration

`config.toml`:

```toml
[tts]
enabled = true
override = "auto"

[tts.elevenlabs]
api_key  = "sk_..."
voice_id = "21m00Tcm4TlvDq8ikWAM"   # Rachel
model    = "eleven_turbo_v2_5"
```

The `[tts.iflytek]` block is removed. `TtsConfig.iflytek` becomes
`TtsConfig.elevenlabs`.

Defaults (`src/config/defaults.rs`):
- `DEFAULT_ELEVENLABS_VOICE_ID = "21m00Tcm4TlvDq8ikWAM"` (Rachel)
- `DEFAULT_ELEVENLABS_MODEL    = "eleven_turbo_v2_5"`

Validation (`Config::validate_tts`): when `enabled && override != Off`, the
only required field is `tts.elevenlabs.api_key`. `voice_id` and `model` have
defaults and don't need validation.

## Speaker implementation

New module `src/tts/elevenlabs.rs`. Replaces `src/tts/iflytek.rs`.

```rust
pub struct ElevenLabsSpeaker {
    cfg: ElevenLabsConfig,
    cache_dir: PathBuf,
    base_url: String,                          // "https://api.elevenlabs.io" in prod
    http: reqwest::Client,
    audio: Option<rodio::OutputStreamHandle>,  // None = cache-only (headless)
    current_sink: Arc<Mutex<Option<rodio::Sink>>>,
    stream_handle: Arc<AtomicU64>,
}
```

Two constructors mirror `IflytekSpeaker`:
- `new(cfg, cache_dir, audio)` — production base URL.
- `with_base_url(cfg, cache_dir, base_url, audio)` — for `mockito` in tests.

### API call

```
POST {base_url}/v1/text-to-speech/{voice_id}
xi-api-key:   {api_key}
Content-Type: application/json
Accept:       audio/mpeg

{
  "text": "<sentence>",
  "model_id": "{model}"
}
```

Response on 200: `audio/mpeg` byte stream containing the full MP3. No
chunked playback — we wait for the full body, write it to cache, then play.

### speak(text) flow

1. `gen = stream_handle.fetch_add(1, SeqCst) + 1` — claim a generation.
2. `path = cache_path_for(text)` — see Cache section.
3. If `path` exists, skip to step 6.
4. POST to ElevenLabs:
   - `200` → atomically write body to `path` (`.mp3.tmp` then rename).
   - `401 | 403` → `TtsError::Auth(message)`.
   - `429 | 5xx` → `TtsError::Network(message)`.
   - Connect/read timeout, DNS failure → `TtsError::Network(message)`.
5. Cancel re-check: `stream_handle.load(SeqCst) != gen` → return `Ok(())`.
6. Read `path` → decode MP3 to PCM. `src/audio/player.rs` already has a
   private `decode_to_pcm(path) -> DecodedPcm` (Symphonia probe + decode,
   handles MP3 and MP4). Bump it and `DecodedPcm` to `pub(crate)` and call
   it from `elevenlabs.rs`. No new dep.
7. Cancel re-check.
8. Build `rodio::Sink`, append the PCM buffer.
9. Under the `current_sink` lock: cancel re-check → drop sink if stale,
   otherwise stop the previous sink and install the new one.
10. Return `Ok(())`. Playback proceeds asynchronously inside rodio.

### cancel()

```rust
pub fn cancel(&self) {
    self.stream_handle.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut guard) = self.current_sink.lock() {
        if let Some(sink) = guard.take() {
            sink.stop();
        }
    }
}
```

Identical pattern to `IflytekSpeaker::cancel`. The generation bump closes the
race where a post-decode install runs after the user already moved to the
next drill.

### Error mapping

| Source                         | `TtsError` variant   |
|--------------------------------|----------------------|
| HTTP 401, 403                  | `Auth(message)`      |
| HTTP 429, 5xx, connect/read IO | `Network(message)`   |
| Cache read/write failure       | `Cache(message)`     |
| MP3 decode failure, rodio error| `Audio(message)`     |
| Generation mismatch mid-call   | early `Ok(())`       |

`MissingCreds` is only emitted by `build_speaker` when `api_key` is empty
(it returns `NullSpeaker`). It does not arise inside `speak()`.

## Caching

`src/tts/cache.rs`:

- Cache key: `sha256(format!("{text}|{voice_id}|{model}"))` hex.
  Changing model (e.g. turbo → multilingual) re-synthesizes —- the timbre
  changes.
- File extension: `.mp3` (was `.wav`).
- `cache_path(dir, key) -> dir/{key}.mp3`.
- `clear_cache(dir)` filter changes from `extension == "wav"` to
  `extension == "mp3"`.

Atomic write uses the same `storage/atomic.rs::write_atomic` helper.

## Speaker factory

`src/tts/speaker.rs::build_speaker`:

```rust
pub fn build_speaker(
    cfg: &ElevenLabsConfig,
    cache_dir: PathBuf,
    mode: TtsOverride,
    audio: Option<rodio::OutputStreamHandle>,
) -> Box<dyn Speaker> {
    if mode == TtsOverride::Off || cfg.api_key.trim().is_empty() {
        return Box::new(NullSpeaker);
    }
    Box::new(ElevenLabsSpeaker::new(cfg.clone(), cache_dir, audio))
}
```

`has_creds` collapses to a single emptiness check on `api_key`.

## Config wizard

`src/ui/config_wizard.rs`:

- `WizardStep::TtsAppId` and `TtsApiSecret` are removed. `TtsApiKey` stays
  and now binds to `tts.elevenlabs.api_key`.
- Total steps: 7 → 5 (Endpoint, ApiKey, Model, TtsEnable, TtsApiKey).
- Step indices renumber accordingly.
- The TTS connectivity test that fires on TtsApiSecret moves to TtsApiKey
  and now hits ElevenLabs. `probe_tts` calls `build_speaker` with the
  draft config and runs `speak("hello")` against a temp cache dir
  (cache-only mode, no audio handle) — fails fast on 401/403. Mirrors the
  iFlytek pattern; cost is ~5 chars of quota per wizard run, acceptable.
- Empty-value error message for TtsApiKey: `"API key cannot be empty"`.

## Doctor

`src/ui/doctor.rs::run_checks`:

The "TTS credentials" check examines only `config.tts.elevenlabs.api_key`.
The pass/warn messaging stays identical.

## TTS status overlay

`src/ui/tts_status.rs` shows provider-specific fields. Replace the iFlytek
labels (app_id, api_key, api_secret, voice) with ElevenLabs labels (api_key
masked, voice_id, model).

## Deletions

- `src/tts/iflytek.rs`
- `src/tts/auth.rs` (iFlytek HMAC-SHA256 WS auth)
- `src/tts/frame.rs` (iFlytek WS frame envelope)
- `src/tts/wav.rs` (WAV writer, no longer needed; cache holds MP3)
- `src/tts/snapshots/` (iFlytek frame/auth golden files)
- `Cargo.toml`: drop `tokio-tungstenite` and `httpdate`.

## Tests

### Unit

- `cache::cache_key` differs when text, voice_id, or model differ.
- `ElevenLabsConfig::default()` carries Rachel + turbo defaults.
- `Config::validate_tts` only flags `tts.elevenlabs.api_key`.
- `build_speaker` returns `NullSpeaker` on empty key or `Off` override;
  `ElevenLabsSpeaker` otherwise.
- Wizard: TtsEnable=Y advances to TtsApiKey directly (no AppId step);
  empty key rejected; non-empty key advances to whatever comes next.

### Speaker behavior (mockito)

- `with_base_url` plus `mockito::Server`:
  - Cache miss → 200 + MP3 body → file written to cache, returns `Ok`.
  - Cache hit → no HTTP call → returns `Ok`.
  - 401 → `TtsError::Auth`.
  - 429 → `TtsError::Network`.
  - cancel() during the HTTP call (mockito with delayed response) → no sink
    installed; subsequent `speak` succeeds normally.

### Manual verify

Run with the user-supplied test key against the live API. Speak one drill,
confirm audio plays. Re-launch, hit cache, confirm no HTTP call (offline
sanity: `INKWORM_LOG=debug` should show cache hit).

## Migration

Existing `~/Documents/InkWorm/config.toml` carries `[tts.iflytek]`. After
upgrade:

1. Config load fails (`deny_unknown_fields`).
2. App falls into the wizard origin path the existing failure handler
   already uses.
3. Wizard rewrites `config.toml` with `[tts.elevenlabs]`.

No special migration code. Old TTS WAV cache files are orphaned; the
user can `/tts clear-cache` (which now targets `.mp3`) — the `.wav`
orphans linger but don't break anything. Doc this in the commit message.

## Out of scope

- Voice cloning, custom voices, voice settings (`stability`, `similarity_boost`)
- Pronunciation dictionaries
- Streaming endpoint (`/stream`)
- Usage / billing dashboards
- Pre-bundled audio (`audio/bundle.rs`) — separate system, untouched.
