//! Data directory resolution for inkworm.
//!
//! Default layout (no overrides) splits user data from volatile / disposable
//! state so per-keystroke saves don't push log lines and lock files through
//! iCloud's sync pipeline:
//!
//! - User data (config/progress/mistakes/stats/courses): `~/Documents/InkWorm`
//!   — kept inside iCloud-synced Documents so multiple Macs share state.
//! - Log file: `~/Library/Logs/InkWorm/inkworm.log` — high-write, single-host.
//! - Cache / lock / failed artifacts: `~/.cache/inkworm/` — disposable.
//!
//! When `--config <path>` (CLI) or `INKWORM_HOME` (env) is set, every file
//! lives under that single override root instead — the override is meant for
//! isolation (smoke runs, tests) where splitting paths would scatter outputs.
//!
//! Resolution priority (highest first):
//!   1. Explicit override (`--config <path>` from CLI) — all paths under override
//!   2. `INKWORM_HOME` environment variable — all paths under override
//!   3. Default split layout above

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DataPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub progress_file: PathBuf,
    pub mistakes_file: PathBuf,
    pub log_file: PathBuf,
    pub lock_file: PathBuf,
    pub stats_file: PathBuf,
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
        if let Some(p) = cli_override {
            return Ok(Self::all_under(p.to_path_buf()));
        }
        if let Some(v) = nonempty_env("INKWORM_HOME") {
            return Ok(Self::all_under(PathBuf::from(v)));
        }
        let home = PathBuf::from(
            nonempty_env("HOME")
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))?,
        );
        Ok(Self::default_split(&home))
    }

    /// Compact layout: every file (including log/lock/cache) lives directly
    /// under `root`. Used for `--config` / `INKWORM_HOME` overrides where
    /// isolation matters more than mac-native locations.
    fn all_under(root: PathBuf) -> Self {
        Self {
            config_file: root.join("config.toml"),
            progress_file: root.join("progress.json"),
            mistakes_file: root.join("mistakes.json"),
            log_file: root.join("inkworm.log"),
            lock_file: root.join("inkworm.lock"),
            stats_file: root.join("stats.json"),
            courses_dir: root.join("courses"),
            failed_dir: root.join("failed"),
            tts_cache_dir: root.join("tts-cache"),
            root,
        }
    }

    /// Default split layout — user data in iCloud Documents, log under
    /// `~/Library/Logs/InkWorm`, cache/lock/failed under `~/.cache/inkworm`.
    fn default_split(home: &Path) -> Self {
        let root = home.join("Documents").join("InkWorm");
        let cache_root = home.join(".cache").join("inkworm");
        let logs_root = home.join("Library").join("Logs").join("InkWorm");
        Self {
            config_file: root.join("config.toml"),
            progress_file: root.join("progress.json"),
            mistakes_file: root.join("mistakes.json"),
            stats_file: root.join("stats.json"),
            courses_dir: root.join("courses"),
            log_file: logs_root.join("inkworm.log"),
            lock_file: cache_root.join("inkworm.lock"),
            failed_dir: cache_root.join("failed"),
            tts_cache_dir: cache_root.join("tts-cache"),
            root,
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.courses_dir)?;
        std::fs::create_dir_all(&self.failed_dir)?;
        std::fs::create_dir_all(&self.tts_cache_dir)?;
        if let Some(p) = self.log_file.parent() {
            std::fs::create_dir_all(p)?;
        }
        if let Some(p) = self.lock_file.parent() {
            std::fs::create_dir_all(p)?;
        }
        Ok(())
    }
}

impl DataPaths {
    pub fn for_tests(root: std::path::PathBuf) -> Self {
        Self::all_under(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_root_sets_mistakes_and_stats_files() {
        let p = DataPaths::for_tests(PathBuf::from("/tmp/inkworm-test"));
        assert_eq!(
            p.mistakes_file,
            PathBuf::from("/tmp/inkworm-test/mistakes.json")
        );
        assert_eq!(p.stats_file, PathBuf::from("/tmp/inkworm-test/stats.json"));
    }
}
