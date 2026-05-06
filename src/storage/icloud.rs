//! iCloud Drive integration for the courses directory.
//!
//! When the user symlinks `courses_dir` into iCloud Drive, files may live as
//! `.foo.json.icloud` placeholders that `read_dir` won't surface. We trigger
//! `brctl download` at startup to materialize them. No-op on non-macOS, and
//! skipped when the path doesn't resolve into the iCloud container.

use std::path::Path;

/// If `p` (after symlink resolution) looks like an iCloud-managed location on
/// macOS, returns the canonical path. Three prefixes count:
///   * `~/Library/Mobile Documents` — the canonical iCloud container.
///   * `~/Documents`, `~/Desktop` — when "Desktop & Documents in iCloud" is
///     enabled these are firmlinks into the container that `canonicalize`
///     does NOT expand, so we must allow-list them by name.
///
/// Always `None` on non-macOS.
pub fn icloud_canonical(p: &Path) -> Option<std::path::PathBuf> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = p;
        None
    }
    #[cfg(target_os = "macos")]
    {
        let real = std::fs::canonicalize(p).ok()?;
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
        let in_icloud = real.starts_with(home.join("Library/Mobile Documents"))
            || real.starts_with(home.join("Documents"))
            || real.starts_with(home.join("Desktop"));
        in_icloud.then_some(real)
    }
}

/// Best-effort: if `courses_dir` is in iCloud, ask the cloud daemon to
/// download all dataless placeholders. Blocking; returns quickly when nothing
/// needs syncing. Errors are swallowed — startup must not fail because of it.
/// stdout/stderr are suppressed so brctl's chatter doesn't leak into the TUI.
///
/// We pass the canonical path because `brctl` does not follow symlinks — it
/// rejects paths that don't textually live inside a CloudDocs library.
pub fn ensure_downloaded(courses_dir: &Path) {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = courses_dir;
    }
    #[cfg(target_os = "macos")]
    {
        let Some(real) = icloud_canonical(courses_dir) else {
            return;
        };
        let _ = std::process::Command::new("brctl")
            .arg("download")
            .arg(&real)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn non_icloud_path_returns_none() {
        // /tmp is not under iCloud
        assert!(icloud_canonical(&PathBuf::from("/tmp")).is_none());
    }

    #[test]
    fn nonexistent_path_returns_none() {
        assert!(icloud_canonical(&PathBuf::from("/this/does/not/exist/anywhere")).is_none());
    }

    #[test]
    fn home_documents_is_treated_as_icloud() {
        // ~/Documents exists on every Mac; we allow-list it because the
        // firmlink form prevents canonicalize from revealing the cloud root.
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        let docs = home.join("Documents");
        if docs.exists() {
            assert!(icloud_canonical(&docs).is_some());
        }
    }
}
