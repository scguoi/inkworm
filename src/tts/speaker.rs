//! Speaker trait, error type, and a `NullSpeaker` fallback.

use std::path::PathBuf;

use async_trait::async_trait;
use thiserror::Error;

use crate::config::{ElevenLabsConfig, TtsOverride};

#[derive(Debug, Error)]
pub enum TtsError {
    #[error("TTS cancelled")]
    Cancelled,
    #[error("TTS auth failure: {0}")]
    Auth(String),
    #[error("TTS network error: {0}")]
    Network(String),
    #[error("audio playback error: {0}")]
    Audio(String),
    #[error("TTS cache error: {0}")]
    Cache(String),
    #[error("TTS credentials missing")]
    MissingCreds,
}

/// The speaker contract. Implementations must be cheap to construct and
/// safe to share across tasks (`Send + Sync`). `speak` is `async` because
/// the real impl will stream over WS; `cancel` is sync because callers
/// need to interrupt immediately (drill-change path).
#[async_trait]
pub trait Speaker: Send + Sync {
    async fn speak(&self, text: &str) -> Result<(), TtsError>;
    fn cancel(&self);
}

/// No-op speaker used when TTS is disabled, credentials are missing,
/// or when audio hardware is unavailable. Both methods succeed silently.
pub struct NullSpeaker;

#[async_trait]
impl Speaker for NullSpeaker {
    async fn speak(&self, _text: &str) -> Result<(), TtsError> {
        Ok(())
    }
    fn cancel(&self) {}
}

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

#[cfg(test)]
fn has_creds(cfg: &ElevenLabsConfig) -> bool {
    !cfg.api_key.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn null_speaker_speak_is_ok() {
        let s = NullSpeaker;
        assert!(s.speak("hello").await.is_ok());
    }

    #[test]
    fn null_speaker_cancel_does_not_panic() {
        let s = NullSpeaker;
        s.cancel();
    }

    #[test]
    fn has_creds_requires_api_key() {
        assert!(!has_creds(&empty_elevenlabs()));
        assert!(has_creds(&full_elevenlabs()));
    }

    #[tokio::test]
    async fn build_speaker_returns_null_when_mode_off() {
        let b = build_speaker(
            &full_elevenlabs(),
            PathBuf::from("/tmp/x"),
            TtsOverride::Off,
            None,
        );
        assert!(b.speak("x").await.is_ok());
    }

    #[tokio::test]
    async fn build_speaker_returns_null_when_creds_missing() {
        let b = build_speaker(
            &empty_elevenlabs(),
            PathBuf::from("/tmp/x"),
            TtsOverride::Auto,
            None,
        );
        assert!(b.speak("x").await.is_ok());
    }
}
