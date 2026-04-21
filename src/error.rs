//! Top-level error enum, covering the full surface area of an inkworm run.
//! User-facing message mapping happens in `ui::error_banner` (later plan).

use thiserror::Error;

use crate::config::ConfigError;
use crate::storage::StorageError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("user cancelled")]
    Cancelled,
}
