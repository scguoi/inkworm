//! Top-level error enum, covering the full surface area of an inkworm run.
//! User-facing message mapping happens in `ui::error_banner` (later plan).

use std::path::PathBuf;
use thiserror::Error;

use crate::config::ConfigError;
use crate::llm::error::LlmError;
use crate::storage::StorageError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("llm error: {0}")]
    Llm(#[from] LlmError),

    #[error("TTS error: {0}")]
    Tts(#[from] crate::tts::speaker::TtsError),

    #[error("reflexion failed after {attempts} attempts; raw saved to {saved_to:?}")]
    Reflexion {
        attempts: u32,
        saved_to: PathBuf,
        summary: String,
    },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("user cancelled")]
    Cancelled,
}
