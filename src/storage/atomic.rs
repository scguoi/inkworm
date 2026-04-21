//! Atomic file write: write to `<path>.tmp`, fsync, rename onto `<path>`.
//!
//! On error (write or fsync), the `.tmp` file is left on disk. This is
//! intentional and safe because the next successful call opens with
//! `truncate(true)`, which overwrites any stale content.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    // Append ".tmp" to the full path (not to the extension). Produces
    // `foo.toml.tmp`, `foo.json.tmp`, or `foo.tmp` without inventing segments.
    let tmp = {
        let mut s = path.as_os_str().to_owned();
        s.push(".tmp");
        PathBuf::from(s)
    };
    {
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    // fsync the directory so the rename is durable on crash.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}
