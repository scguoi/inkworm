//! Default constants for Config.

pub const DEFAULT_LLM_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_LLM_MODEL: &str = "gpt-4o-mini";
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_REFLEXION_BUDGET_SECS: u64 = 60;
pub const DEFAULT_MAX_CONCURRENT_CALLS: usize = 5;
pub const DEFAULT_MAX_ARTICLE_BYTES: usize = 16384;
pub const DEFAULT_IFLYTEK_VOICE: &str = "x3_catherine";
