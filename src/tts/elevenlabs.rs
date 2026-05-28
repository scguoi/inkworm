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
