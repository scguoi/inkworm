//! Data directory resolution for inkworm.
//!
//! Resolution priority (highest first):
//!   1. Explicit override (`--config <path>` from CLI)
//!   2. `INKWORM_HOME` environment variable
//!   3. `XDG_CONFIG_HOME/inkworm`
//!   4. `~/.config/inkworm`

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DataPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub progress_file: PathBuf,
    pub mistakes_file: PathBuf,
    pub log_file: PathBuf,
    pub courses_dir: PathBuf,
    pub failed_dir: PathBuf,
    pub tts_cache_dir: PathBuf,
}

/// Reads an environment variable, treating both unset and empty string as absent.
fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

impl DataPaths {
    pub fn resolve(cli_override: Option<&Path>) -> std::io::Result<Self> {
        let root = if let Some(p) = cli_override {
            p.to_path_buf()
        } else if let Some(v) = nonempty_env("INKWORM_HOME") {
            PathBuf::from(v)
        } else if let Some(v) = nonempty_env("XDG_CONFIG_HOME") {
            PathBuf::from(v).join("inkworm")
        } else {
            let home = nonempty_env("HOME")
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))?;
            PathBuf::from(home).join(".config").join("inkworm")
        };
        Ok(Self::from_root(root))
    }

    fn from_root(root: PathBuf) -> Self {
        Self {
            config_file: root.join("config.toml"),
            progress_file: root.join("progress.json"),
            mistakes_file: root.join("mistakes.json"),
            log_file: root.join("inkworm.log"),
            courses_dir: root.join("courses"),
            failed_dir: root.join("failed"),
            tts_cache_dir: root.join("tts-cache"),
            root,
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.courses_dir)?;
        std::fs::create_dir_all(&self.failed_dir)?;
        std::fs::create_dir_all(&self.tts_cache_dir)?;
        Ok(())
    }
}

impl DataPaths {
    pub fn for_tests(root: std::path::PathBuf) -> Self {
        Self::from_root(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_root_sets_mistakes_file() {
        let p = DataPaths::for_tests(PathBuf::from("/tmp/inkworm-test"));
        assert_eq!(p.mistakes_file, PathBuf::from("/tmp/inkworm-test/mistakes.json"));
    }
}
