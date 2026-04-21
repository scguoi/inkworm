//! Configuration loading, validation, and persistence.

pub mod defaults;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::storage::atomic::write_atomic;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub generation: GenerationConfig,
    #[serde(default)]
    pub tts: TtsConfig,
    #[serde(default)]
    pub data: DataConfig,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,
    #[serde(default = "default_reflexion_budget")]
    pub reflexion_budget_secs: u64,
}

fn default_base_url() -> String {
    defaults::DEFAULT_LLM_BASE_URL.into()
}
fn default_model() -> String {
    defaults::DEFAULT_LLM_MODEL.into()
}
fn default_request_timeout() -> u64 {
    defaults::DEFAULT_REQUEST_TIMEOUT_SECS
}
fn default_reflexion_budget() -> u64 {
    defaults::DEFAULT_REFLEXION_BUDGET_SECS
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: String::new(),
            model: default_model(),
            request_timeout_secs: default_request_timeout(),
            reflexion_budget_secs: default_reflexion_budget(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationConfig {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_calls: usize,
    #[serde(default = "default_max_article")]
    pub max_article_bytes: usize,
}

fn default_max_concurrent() -> usize {
    defaults::DEFAULT_MAX_CONCURRENT_CALLS
}
fn default_max_article() -> usize {
    defaults::DEFAULT_MAX_ARTICLE_BYTES
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            max_concurrent_calls: default_max_concurrent(),
            max_article_bytes: default_max_article(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TtsConfig {
    #[serde(default = "default_tts_enabled")]
    pub enabled: bool,
    #[serde(default = "default_tts_override")]
    pub r#override: TtsOverride,
    #[serde(default)]
    pub iflytek: IflytekConfig,
}

fn default_tts_enabled() -> bool {
    true
}
fn default_tts_override() -> TtsOverride {
    TtsOverride::Auto
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: default_tts_enabled(),
            r#override: default_tts_override(),
            iflytek: IflytekConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TtsOverride {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IflytekConfig {
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default = "default_voice")]
    pub voice: String,
}

fn default_voice() -> String {
    defaults::DEFAULT_IFLYTEK_VOICE.into()
}

impl Default for IflytekConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            api_key: String::new(),
            api_secret: String::new(),
            voice: default_voice(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DataConfig {
    #[serde(default)]
    pub home: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            llm: LlmConfig::default(),
            generation: GenerationConfig::default(),
            tts: TtsConfig::default(),
            data: DataConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid field {field}: {reason}")]
    Invalid { field: &'static str, reason: String },
    #[error("io: {0}")]
    Io(String),
    #[error("toml: {0}")]
    Toml(String),
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| ConfigError::Io(format!("{path:?}: {e}")))?;
        toml::from_str(&text).map_err(|e| ConfigError::Toml(e.to_string()))
    }

    pub fn write_atomic(&self, path: &Path) -> Result<(), ConfigError> {
        let text = toml::to_string_pretty(self).map_err(|e| ConfigError::Toml(e.to_string()))?;
        write_atomic(path, text.as_bytes()).map_err(|e| ConfigError::Io(e.to_string()))?;
        Ok(())
    }

    /// Collects ALL validation errors, does not short-circuit.
    pub fn validate(&self) -> Vec<ConfigError> {
        let mut errs = Vec::new();
        if self.llm.api_key.trim().is_empty() {
            errs.push(ConfigError::MissingField("llm.api_key"));
        }
        if self.llm.base_url.trim().is_empty() {
            errs.push(ConfigError::MissingField("llm.base_url"));
        }
        if self.llm.model.trim().is_empty() {
            errs.push(ConfigError::MissingField("llm.model"));
        }
        if self.tts.enabled && self.tts.r#override != TtsOverride::Off {
            if self.tts.iflytek.app_id.trim().is_empty() {
                errs.push(ConfigError::MissingField("tts.iflytek.app_id"));
            }
            if self.tts.iflytek.api_key.trim().is_empty() {
                errs.push(ConfigError::MissingField("tts.iflytek.api_key"));
            }
            if self.tts.iflytek.api_secret.trim().is_empty() {
                errs.push(ConfigError::MissingField("tts.iflytek.api_secret"));
            }
        }
        if self.generation.max_concurrent_calls == 0 {
            errs.push(ConfigError::Invalid {
                field: "generation.max_concurrent_calls",
                reason: "must be ≥1".into(),
            });
        }
        errs
    }

    pub fn data_home_override(&self) -> Option<PathBuf> {
        let s = self.data.home.trim();
        if s.is_empty() {
            None
        } else {
            Some(PathBuf::from(s))
        }
    }
}
