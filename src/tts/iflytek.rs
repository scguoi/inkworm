//! Live iFlytek TTS speaker — WS streaming + WAV cache + optional rodio playback.
//!
//! Task 4 will fill in the WS miss path; this task ships the scaffold and the
//! cache-hit branch. Cancellation plumbing (`stream_handle`) is wired here so
//! Task 4 only has to populate it during the WS flow.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
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
    /// Stores the most recent rodio Sink so `cancel()` can stop mid-playback.
    /// `None` when no audio has been queued yet or when `audio` is `None`.
    current_sink: Arc<Mutex<Option<rodio::Sink>>>,
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
            current_sink: Arc::new(Mutex::new(None)),
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
            current_sink: Arc::new(Mutex::new(None)),
        }
    }

    fn cache_path_for(&self, text: &str) -> PathBuf {
        let key = cache::cache_key(text, &self.cfg.voice);
        cache::cache_path(&self.cache_dir, &key)
    }

    /// Queue samples for playback via rodio, if an audio handle is present.
    /// Returns Ok(()) (no-op) when `self.audio` is None.
    fn play_pcm(&self, samples: Vec<i16>) -> Result<(), TtsError> {
        let Some(handle) = &self.audio else {
            return Ok(());
        };
        let sink = rodio::Sink::try_new(handle).map_err(|e| TtsError::Audio(e.to_string()))?;
        sink.append(rodio::buffer::SamplesBuffer::new(
            wav::CHANNELS,
            wav::SAMPLE_RATE,
            samples,
        ));
        // Store the sink so `cancel()` can stop playback mid-audio. The
        // previous sink (if any) is dropped here — `rodio::Sink` Drop does
        // NOT stop playback, it just lets the audio finish naturally. For
        // v1 that's fine: the old audio was from a prior drill that either
        // finished already or was explicitly stopped via `cancel`.
        if let Ok(mut guard) = self.current_sink.lock() {
            *guard = Some(sink);
        }
        Ok(())
    }

    async fn stream_ws(&self, text: &str, cancel: CancellationToken) -> Result<Vec<i16>, TtsError> {
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

    fn authorized_url(&self, now: SystemTime) -> String {
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
        let text_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let start = std::time::Instant::now();
        let path = self.cache_path_for(text);
        if path.exists() {
            let samples = wav::read_wav_pcm(&path)
                .map_err(|e| TtsError::Cache(format!("cache read: {e}")))?;
            let duration_ms = start.elapsed().as_millis();
            tracing::info!(
                text_hash = %text_hash,
                cache_hit = true,
                duration_ms = duration_ms,
                "TTS cache hit"
            );
            return self.play_pcm(samples);
        }

        // Register a fresh cancel token before any WS work, so `cancel`
        // can interrupt mid-stream. Supersede any prior in-flight token.
        let token = CancellationToken::new();
        if let Ok(mut guard) = self.stream_handle.lock() {
            if let Some(prev) = guard.replace(token.clone()) {
                prev.cancel();
            }
        }

        let samples = self.stream_ws(text, token).await?;

        // Clear the handle on clean completion.
        if let Ok(mut guard) = self.stream_handle.lock() {
            *guard = None;
        }

        let duration_ms = start.elapsed().as_millis();
        tracing::info!(
            text_hash = %text_hash,
            cache_hit = false,
            duration_ms = duration_ms,
            "TTS synthesis completed"
        );

        // Only cache full recordings. Cache-write failure is non-fatal:
        // we still want to play the audio we have in memory.
        if !samples.is_empty() {
            if let Err(e) = wav::write_wav_atomic(&path, &samples) {
                eprintln!("tts cache write failed for {}: {e}", path.display());
            }
        }

        self.play_pcm(samples)
    }

    fn cancel(&self) {
        // Cancel any in-flight WS stream.
        if let Ok(mut guard) = self.stream_handle.lock() {
            if let Some(token) = guard.take() {
                token.cancel();
            }
        }
        // Stop any currently-playing audio.
        if let Ok(mut guard) = self.current_sink.lock() {
            if let Some(sink) = guard.take() {
                sink.stop();
            }
        }
    }
}

/// Map iFlytek error codes onto `TtsError`. Any auth-cluster or quota-cluster
/// code → Auth; everything else → Network.
fn classify_iflytek_code(code: i32, message: &str) -> TtsError {
    let auth_like = matches!(code, 10105 | 10106 | 10107 | 10110 | 11200..=11210);
    let msg = format!("iflytek {code}: {message}");
    if auth_like {
        TtsError::Auth(msg)
    } else {
        TtsError::Network(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let text = "hello cache";
        let path = speaker.cache_path_for(text);
        wav::write_wav_atomic(&path, &[0, 1, 2, 3, 4]).unwrap();

        let res = speaker.speak(text).await;
        assert!(res.is_ok(), "{res:?}");
    }

    #[tokio::test]
    async fn cache_miss_without_server_reachable_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let speaker = IflytekSpeaker::with_base_url(
            dummy_cfg(),
            tmp.path().to_path_buf(),
            None,
            "ws://127.0.0.1:1/".into(), // port 1 is reserved + usually closed
        );
        let err = speaker.speak("miss").await.unwrap_err();
        assert!(matches!(err, TtsError::Network(_)), "got {err:?}");
    }

    #[test]
    fn cancel_without_active_speak_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let speaker = IflytekSpeaker::new(dummy_cfg(), tmp.path().to_path_buf(), None);
        speaker.cancel(); // must not panic
    }
}
