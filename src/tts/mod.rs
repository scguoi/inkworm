//! TTS subsystem root. Plan 6a lands only the cache-clear helper here;
//! Plan 6b will add the Speaker trait, IflytekSpeaker, device detection,
//! and rodio playback.

use std::fs;
use std::io;
use std::path::Path;

/// Delete every regular file inside `dir` whose extension is `wav`.
/// Returns the number of files removed.
/// Leaves the directory itself (and any subdirectories) in place.
/// If `dir` does not exist, returns `Ok(0)` — nothing to clear.
pub fn clear_cache(dir: &Path) -> io::Result<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0usize;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }
        fs::remove_file(&path)?;
        removed += 1;
    }
    Ok(removed)
}

pub mod auth;
pub mod cache;
pub mod wav;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_cache_missing_dir_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let nope = tmp.path().join("nonexistent");
        assert_eq!(clear_cache(&nope).unwrap(), 0);
    }

    #[test]
    fn clear_cache_empty_dir_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(clear_cache(tmp.path()).unwrap(), 0);
    }

    #[test]
    fn clear_cache_removes_only_wav_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.wav"), b"fake").unwrap();
        std::fs::write(tmp.path().join("b.wav"), b"fake").unwrap();
        std::fs::write(tmp.path().join("c.wav"), b"fake").unwrap();
        std::fs::write(tmp.path().join("notes.txt"), b"keep me").unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();

        let removed = clear_cache(tmp.path()).unwrap();
        assert_eq!(removed, 3);
        assert!(tmp.path().join("notes.txt").exists());
        assert!(tmp.path().join("sub").is_dir());
        assert!(!tmp.path().join("a.wav").exists());
    }
}
