//! Path resolution for course-bundled audio files.
//!
//! Layout (per spec §2):
//! `<courses_dir>/<yyyy-mm>/<id_tail>/s{order:02}-d{stage}.mp3`
//! where `id_tail` is everything after `yyyy-mm-dd-` (i.e. `id[8..]`).

use std::path::{Path, PathBuf};

use crate::storage::course::{Course, StorageError};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

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

/// On Unix, returns `true` iff `path.metadata().blocks() > 0`. A
/// regular file that exists logically but has zero physical blocks is
/// an iCloud `dataless` placeholder on macOS — opening it forces a
/// synchronous network fetch. On non-Unix targets there is no such
/// notion, so this function returns `true` for any path whose metadata
/// reads successfully.
///
/// Returns `false` on metadata errors (missing path, permission, etc.).
pub fn is_locally_resident(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        meta.blocks() > 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

/// Enumerate every drill's bundled mp3 path for `course`. Returns
/// empty Vec when the course id lacks the `yyyy-mm-dd-` prefix.
pub fn bundle_paths_for_course(courses_dir: &Path, course: &Course) -> Vec<PathBuf> {
    course
        .sentences
        .iter()
        .flat_map(|s| {
            let order = s.order;
            s.drills
                .iter()
                .map(move |d| (order, d.stage))
                .filter_map(|(order, stage)| {
                    bundle_path(courses_dir, &course.id, order, stage).ok()
                })
        })
        .collect()
}

/// Open `path` and drain its bytes into a sink. On macOS this forces
/// iCloud Drive to materialize a `dataless` placeholder. Returns the
/// number of bytes read.
pub(crate) fn materialize(path: &Path) -> std::io::Result<u64> {
    let mut file = std::fs::File::open(path)?;
    let mut sink = std::io::sink();
    std::io::copy(&mut file, &mut sink)
}

/// Fire-and-forget: spawn a tokio task that materializes every drill
/// mp3 for `course` from iCloud, sequentially, in a blocking pool.
/// Per-file failures are logged at `debug` and never propagate.
///
/// Silently no-ops if called outside a tokio runtime (e.g. from sync
/// integration tests that construct `App` directly).
pub fn spawn_prewarm_course(courses_dir: PathBuf, course: &Course) {
    let paths = bundle_paths_for_course(&courses_dir, course);
    if paths.is_empty() {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        for path in paths {
            let p = path.clone();
            let join = tokio::task::spawn_blocking(move || materialize(&p)).await;
            match join {
                Ok(Ok(n)) => tracing::debug!("bundle prewarm {}: {} bytes", path.display(), n),
                Ok(Err(e)) => tracing::debug!("bundle prewarm skip {}: {}", path.display(), e),
                Err(e) => tracing::warn!("bundle prewarm join error for {}: {}", path.display(), e),
            }
        }
    });
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
    use crate::storage::course::{Course, Drill, Focus, Sentence, Source, SourceKind};

    fn course_with_drills(id: &str, drills_per_sentence: &[&[u32]]) -> Course {
        use chrono::TimeZone;
        let sentences = drills_per_sentence
            .iter()
            .enumerate()
            .map(|(i, stages)| Sentence {
                order: (i + 1) as u32,
                drills: stages
                    .iter()
                    .map(|&stage| Drill {
                        stage,
                        focus: Focus::Keywords,
                        chinese: "测".into(),
                        english: "t".into(),
                        soundmark: "/t/".into(),
                    })
                    .collect(),
            })
            .collect();
        Course {
            schema_version: 2,
            id: id.into(),
            title: "T".into(),
            description: None,
            source: Source {
                kind: SourceKind::Manual,
                url: String::new(),
                created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 6, 0, 0, 0).unwrap(),
                model: "t".into(),
            },
            sentences,
        }
    }

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

    #[test]
    fn bundle_paths_for_course_enumerates_every_drill() {
        let course = course_with_drills("2026-05-06-foo", &[&[1, 2, 3, 4], &[1, 2]]);
        let paths = bundle_paths_for_course(Path::new("/c"), &course);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/c/2026-05/06-foo/s01-d1.mp3"),
                PathBuf::from("/c/2026-05/06-foo/s01-d2.mp3"),
                PathBuf::from("/c/2026-05/06-foo/s01-d3.mp3"),
                PathBuf::from("/c/2026-05/06-foo/s01-d4.mp3"),
                PathBuf::from("/c/2026-05/06-foo/s02-d1.mp3"),
                PathBuf::from("/c/2026-05/06-foo/s02-d2.mp3"),
            ]
        );
    }

    #[test]
    fn bundle_paths_for_course_invalid_id_returns_empty() {
        let course = course_with_drills("no-date-prefix", &[&[1]]);
        let paths = bundle_paths_for_course(Path::new("/c"), &course);
        assert!(paths.is_empty(), "got {paths:?}");
    }

    #[test]
    fn materialize_reads_real_file_returns_byte_count() {
        let bytes = materialize(Path::new("fixtures/audio/silence.mp3")).unwrap();
        assert!(bytes > 0, "expected nonzero, got {bytes}");
    }

    #[test]
    fn spawn_prewarm_course_outside_runtime_does_not_panic() {
        let course = course_with_drills("2026-05-06-foo", &[&[1]]);
        // Called from a plain #[test] = no tokio runtime in scope.
        spawn_prewarm_course(PathBuf::from("/tmp/no-such-courses"), &course);
    }

    #[test]
    fn materialize_missing_file_returns_io_error() {
        let err = materialize(Path::new("/definitely/does/not/exist.mp3")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn is_locally_resident_false_for_zero_byte_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("placeholder.mp3");
        std::fs::write(&p, b"").unwrap();
        assert!(!is_locally_resident(&p));
    }

    #[test]
    fn is_locally_resident_true_for_nonempty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("real.mp3");
        std::fs::write(&p, b"some bytes").unwrap();
        assert!(is_locally_resident(&p));
    }

    #[test]
    fn is_locally_resident_false_for_missing_path() {
        assert!(!is_locally_resident(std::path::Path::new(
            "/definitely/does/not/exist.mp3",
        )));
    }
}
