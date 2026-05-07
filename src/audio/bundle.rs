//! Path resolution for course-bundled audio files.
//!
//! Layout (per spec §2):
//! `<courses_dir>/<yyyy-mm>/<id_tail>/s{order:02}-d{stage}.mp3`
//! where `id_tail` is everything after `yyyy-mm-dd-` (i.e. `id[8..]`).

use std::path::{Path, PathBuf};

use crate::storage::course::StorageError;

/// Resolve the on-disk path for a single drill's bundled mp3.
///
/// Returns `StorageError::InvalidId` when `course_id` does not begin
/// with the `yyyy-mm-dd-` prefix. Does NOT check whether the file
/// exists — use `bundle_exists` for that.
pub fn bundle_path(
    courses_dir: &Path,
    course_id: &str,
    order: u32,
    stage: u32,
) -> Result<PathBuf, StorageError> {
    if !has_yyyy_mm_dd_prefix(course_id) {
        return Err(StorageError::InvalidId(course_id.to_string()));
    }
    let yyyy_mm = &course_id[0..7]; // "2026-05"
    let id_tail = &course_id[8..]; // "06-foo-bar"
    let file = format!("s{:02}-d{}.mp3", order, stage);
    Ok(courses_dir.join(yyyy_mm).join(id_tail).join(file))
}

/// Convenience: returns `true` iff `bundle_path` resolves AND the file
/// exists. Any error (invalid id, IO error) maps to `false`.
pub fn bundle_exists(courses_dir: &Path, course_id: &str, order: u32, stage: u32) -> bool {
    match bundle_path(courses_dir, course_id, order, stage) {
        Ok(p) => p.is_file(),
        Err(_) => false,
    }
}

fn has_yyyy_mm_dd_prefix(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 11
        && b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && b[10] == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_path_yyyy_mm_split() {
        let p = bundle_path(Path::new("/tmp/courses"), "2026-05-06-foo", 1, 1).unwrap();
        assert_eq!(p, PathBuf::from("/tmp/courses/2026-05/06-foo/s01-d1.mp3"));
    }

    #[test]
    fn bundle_path_pads_order_to_two_digits() {
        let p = bundle_path(Path::new("/c"), "2026-05-06-x", 9, 1).unwrap();
        assert!(p.ends_with("s09-d1.mp3"), "got {p:?}");
        let p = bundle_path(Path::new("/c"), "2026-05-06-x", 12, 3).unwrap();
        assert!(p.ends_with("s12-d3.mp3"), "got {p:?}");
    }

    #[test]
    fn bundle_path_invalid_id_errors() {
        let err = bundle_path(Path::new("/c"), "no-prefix", 1, 1).unwrap_err();
        assert!(matches!(err, StorageError::InvalidId(_)), "got {err:?}");
    }

    #[test]
    fn bundle_exists_false_when_dir_missing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!bundle_exists(tmp.path(), "2026-05-06-foo", 1, 1));
    }

    #[test]
    fn bundle_exists_true_when_file_present() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("2026-05").join("06-foo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s01-d1.mp3"), b"").unwrap();
        assert!(bundle_exists(tmp.path(), "2026-05-06-foo", 1, 1));
    }

    #[test]
    fn bundle_exists_false_for_other_stage() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("2026-05").join("06-foo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s01-d1.mp3"), b"").unwrap();
        assert!(!bundle_exists(tmp.path(), "2026-05-06-foo", 1, 2));
    }

    #[test]
    fn bundle_exists_false_for_invalid_id() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!bundle_exists(tmp.path(), "no-prefix", 1, 1));
    }
}
