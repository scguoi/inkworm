//! Speaker trait, error type, and a `NullSpeaker` fallback.
//! `IflytekSpeaker` lands in Plan 6c.

use std::path::PathBuf;

use async_trait::async_trait;
use thiserror::Error;

use crate::config::{IflytekConfig, TtsOverride};

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
///
/// Plan 6b: always returns `NullSpeaker`. Plan 6c will swap in
/// `IflytekSpeaker` when credentials are present and override ≠ Off.
pub fn build_speaker(
    cfg: &IflytekConfig,
    _cache_dir: PathBuf,
    mode: TtsOverride,
) -> Box<dyn Speaker> {
    if mode == TtsOverride::Off || !has_creds(cfg) {
        return Box::new(NullSpeaker);
    }
    // Plan 6c: replace with `Box::new(IflytekSpeaker::new(cfg.clone(), cache_dir))`.
    Box::new(NullSpeaker)
}

fn has_creds(cfg: &IflytekConfig) -> bool {
    !cfg.app_id.trim().is_empty()
        && !cfg.api_key.trim().is_empty()
        && !cfg.api_secret.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_iflytek() -> IflytekConfig {
        IflytekConfig {
            app_id: String::new(),
            api_key: String::new(),
            api_secret: String::new(),
            voice: "x3_catherine".into(),
        }
    }

    fn full_iflytek() -> IflytekConfig {
        IflytekConfig {
            app_id: "app".into(),
            api_key: "k".into(),
            api_secret: "s".into(),
            voice: "x3_catherine".into(),
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
    fn has_creds_requires_all_three_nonempty() {
        let mut cfg = full_iflytek();
        assert!(has_creds(&cfg));
        cfg.app_id = "   ".into();
        assert!(!has_creds(&cfg));
        cfg = full_iflytek();
        cfg.api_key.clear();
        assert!(!has_creds(&cfg));
        cfg = full_iflytek();
        cfg.api_secret = "\t".into();
        assert!(!has_creds(&cfg));
    }

    #[tokio::test]
    async fn build_speaker_returns_null_when_mode_off() {
        let b = build_speaker(&full_iflytek(), PathBuf::from("/tmp/x"), TtsOverride::Off);
        assert!(b.speak("x").await.is_ok());
    }

    #[tokio::test]
    async fn build_speaker_returns_null_when_creds_missing() {
        let b = build_speaker(&empty_iflytek(), PathBuf::from("/tmp/x"), TtsOverride::Auto);
        assert!(b.speak("x").await.is_ok());
    }

    #[tokio::test]
    async fn build_speaker_returns_null_when_creds_present_but_plan6b() {
        let b = build_speaker(&full_iflytek(), PathBuf::from("/tmp/x"), TtsOverride::On);
        assert!(b.speak("x").await.is_ok());
    }
}
