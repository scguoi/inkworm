//! Single-instance advisory lock.
//!
//! Prevents two `inkworm` processes from running against the same data
//! root concurrently. Without this guard, both processes load Progress /
//! Mistakes at startup, edit independently, and on exit the second-to-quit
//! process blindly overwrites the file with its stale snapshot, silently
//! discarding the other's work.
//!
//! Uses `flock(2)` via `fs2`. Locks are tied to the open file description,
//! so they are released automatically when the process exits (even on
//! SIGKILL / crash) — no stale-lock recovery needed.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use fs2::FileExt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstanceLockError {
    #[error("another inkworm instance is running")]
    AlreadyRunning { pid: Option<u32> },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Holds the exclusive flock for the process lifetime. Drop releases it.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Try to acquire the lock at `path`. Returns `AlreadyRunning` (with
    /// the other instance's PID, if readable) when another process holds
    /// it. The lock file is created if missing.
    pub fn try_acquire(path: &Path) -> Result<Self, InstanceLockError> {
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;

        match file.try_lock_exclusive() {
            Ok(()) => {
                let pid = std::process::id();
                let _ = file.set_len(0);
                let _ = writeln!(file, "{pid}");
                let _ = file.sync_all();
                Ok(Self { _file: file })
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                Err(InstanceLockError::AlreadyRunning {
                    pid: read_pid(path),
                })
            }
            Err(e) => Err(InstanceLockError::Io(e)),
        }
    }
}

fn read_pid(path: &Path) -> Option<u32> {
    let mut buf = String::new();
    File::open(path).ok()?.read_to_string(&mut buf).ok()?;
    buf.trim().parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn first_acquire_succeeds() {
        let d = tempdir().unwrap();
        let p = d.path().join("test.lock");
        let _g = InstanceLock::try_acquire(&p).expect("first acquire should succeed");
    }

    #[test]
    fn second_acquire_fails_while_first_held() {
        let d = tempdir().unwrap();
        let p = d.path().join("test.lock");
        let _g = InstanceLock::try_acquire(&p).expect("first acquire");
        let err = InstanceLock::try_acquire(&p).expect_err("second acquire must fail");
        match err {
            InstanceLockError::AlreadyRunning { pid } => {
                assert_eq!(
                    pid,
                    Some(std::process::id()),
                    "lock file should record holder pid"
                );
            }
            other => panic!("expected AlreadyRunning, got {other:?}"),
        }
    }

    #[test]
    fn second_acquire_succeeds_after_first_drops() {
        let d = tempdir().unwrap();
        let p = d.path().join("test.lock");
        let g = InstanceLock::try_acquire(&p).expect("first acquire");
        drop(g);
        let _g2 = InstanceLock::try_acquire(&p).expect("second acquire after drop");
    }

    #[test]
    fn lock_file_contains_current_pid_after_acquire() {
        let d = tempdir().unwrap();
        let p = d.path().join("test.lock");
        let _g = InstanceLock::try_acquire(&p).expect("acquire");
        let body = std::fs::read_to_string(&p).unwrap();
        assert_eq!(body.trim().parse::<u32>().ok(), Some(std::process::id()));
    }
}
