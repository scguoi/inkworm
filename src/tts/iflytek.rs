//! Live iFlytek TTS speaker — WS streaming + WAV cache + optional rodio playback.
//!
//! Task 4 will fill in the WS miss path; this task ships the scaffold and the
//! cache-hit branch. Cancellation plumbing (`stream_handle`) is wired here so
//! Task 4 only has to populate it during the WS flow.

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
        let Some(handle) = &self.audio else {
            return Ok(());
        };
        let sink = rodio::Sink::try_new(handle).map_err(|e| TtsError::Audio(e.to_string()))?;
        sink.append(rodio::buffer::SamplesBuffer::new(
            wav::CHANNELS,
            wav::SAMPLE_RATE,
            samples,
        ));
        // Detach: rodio keeps the audio queued in a background thread even
        // after we drop the Sink handle. We trade loss of mid-playback cancel
        // (Plan 6d will add a persisted Sink field) for a simpler v1.
        std::mem::forget(sink);
        Ok(())
    }

    /// Also used by Task 4's WS miss path — suppress unused warnings until then.
    #[allow(dead_code)]
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
        let path = self.cache_path_for(text);
        // Cache hit: read WAV, queue playback, done.
        if path.exists() {
            let samples = wav::read_wav_pcm(&path)
                .map_err(|e| TtsError::Cache(format!("cache read: {e}")))?;
            return self.play_pcm(samples);
        }
        // Cache miss: Task 4 fills this in. Loud error until then.
        Err(TtsError::Network(
            "WS path not implemented yet (Task 4)".into(),
        ))
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
