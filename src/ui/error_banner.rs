//! User-facing error messages for AppError variants.

use crate::error::AppError;
use crate::llm::error::LlmError;
use crate::tts::speaker::TtsError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessage {
    pub headline: String,
    pub hint: String,
    pub severity: Severity,
}

impl UserMessage {
    fn new(headline: impl Into<String>, hint: impl Into<String>, severity: Severity) -> Self {
        Self {
            headline: headline.into(),
            hint: hint.into(),
            severity,
        }
    }
}

pub fn user_message(err: &AppError) -> UserMessage {
    match err {
        AppError::Llm(llm_err) => match llm_err {
            LlmError::Unauthorized => UserMessage::new(
                "Authentication failed",
                "Check your API key in config",
                Severity::Error,
            ),
            LlmError::Network(_) => UserMessage::new(
                "Network error",
                "Check your internet connection",
                Severity::Error,
            ),
            LlmError::Timeout(_) => UserMessage::new(
                "Request timed out",
                "Try again or check your endpoint",
                Severity::Error,
            ),
            LlmError::RateLimited(_) => UserMessage::new(
                "Rate limited",
                "Wait a moment and try again",
                Severity::Warning,
            ),
            LlmError::Server { .. } => UserMessage::new(
                "Server error",
                "The API returned an error, try again",
                Severity::Error,
            ),
            LlmError::InvalidResponse(_) => UserMessage::new(
                "Response parse error",
                "The API returned invalid data",
                Severity::Error,
            ),
            LlmError::Cancelled => UserMessage::new("Cancelled", "", Severity::Info),
        },
        AppError::Reflexion { .. } => UserMessage::new(
            "Course generation failed",
            "LLM couldn't produce valid output after 3 attempts",
            Severity::Error,
        ),
        AppError::Io(_) => UserMessage::new(
            "File system error",
            "Check disk space and permissions",
            Severity::Error,
        ),
        AppError::Cancelled => UserMessage::new("Cancelled", "", Severity::Info),
        AppError::Config(_) => {
            UserMessage::new("Configuration error", "Run /config to fix", Severity::Error)
        }
        AppError::Storage(_) => UserMessage::new(
            "Storage error",
            "Check data directory permissions",
            Severity::Error,
        ),
        AppError::Tts(tts_err) => match tts_err {
            TtsError::Auth(_) => UserMessage::new(
                "TTS authentication failed",
                "Check your iFlytek credentials",
                Severity::Error,
            ),
            TtsError::Network(_) => UserMessage::new(
                "TTS network error",
                "Check your internet connection",
                Severity::Error,
            ),
            TtsError::Audio(_) => UserMessage::new(
                "Audio playback error",
                "Check your audio device",
                Severity::Error,
            ),
            TtsError::Cache(_) => UserMessage::new(
                "TTS cache error",
                "Check disk space and permissions",
                Severity::Error,
            ),
            TtsError::MissingCreds => UserMessage::new(
                "TTS credentials missing",
                "Run /config to set up TTS",
                Severity::Error,
            ),
            TtsError::Cancelled => UserMessage::new("TTS cancelled", "", Severity::Info),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigError;
    use crate::error::AppError;
    use crate::llm::error::LlmError;
    use crate::storage::StorageError;
    use std::path::PathBuf;
    use std::time::Duration;

    fn assert_non_empty_headline(err: AppError) {
        let msg = user_message(&err);
        assert!(!msg.headline.is_empty(), "headline empty for {err:?}");
    }

    #[test]
    fn all_variants_have_non_empty_headline() {
        // LLM variants (skip Network — reqwest::Error is not constructible in unit tests)
        assert_non_empty_headline(AppError::Llm(LlmError::Unauthorized));
        assert_non_empty_headline(AppError::Llm(LlmError::Timeout(Duration::from_secs(30))));
        assert_non_empty_headline(AppError::Llm(LlmError::RateLimited(None)));
        assert_non_empty_headline(AppError::Llm(LlmError::Server {
            status: 500,
            body: "oops".into(),
        }));
        assert_non_empty_headline(AppError::Llm(LlmError::InvalidResponse("bad".into())));
        assert_non_empty_headline(AppError::Llm(LlmError::Cancelled));

        // Reflexion
        assert_non_empty_headline(AppError::Reflexion {
            attempts: 3,
            saved_to: PathBuf::from("/tmp/raw.json"),
            summary: "failed".into(),
        });

        // Io
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        assert_non_empty_headline(AppError::Io(io_err));

        // Cancelled
        assert_non_empty_headline(AppError::Cancelled);

        // Config
        assert_non_empty_headline(AppError::Config(ConfigError::MissingField("llm.api_key")));

        // Storage
        assert_non_empty_headline(AppError::Storage(StorageError::NotFound("x".into())));

        // TTS
        assert_non_empty_headline(AppError::Tts(crate::tts::speaker::TtsError::Auth(
            "bad".into(),
        )));
        assert_non_empty_headline(AppError::Tts(crate::tts::speaker::TtsError::Network(
            "timeout".into(),
        )));
        assert_non_empty_headline(AppError::Tts(crate::tts::speaker::TtsError::Audio(
            "no device".into(),
        )));
        assert_non_empty_headline(AppError::Tts(crate::tts::speaker::TtsError::Cache(
            "disk full".into(),
        )));
        assert_non_empty_headline(AppError::Tts(crate::tts::speaker::TtsError::MissingCreds));
        assert_non_empty_headline(AppError::Tts(crate::tts::speaker::TtsError::Cancelled));
    }
}
