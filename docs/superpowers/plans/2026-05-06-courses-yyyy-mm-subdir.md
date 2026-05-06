# Courses YYYY-MM Subdirectory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reorganize on-disk courses into `courses/yyyy-mm/dd-<rest>.json` with a one-shot startup migration, while keeping `course.id` (and all existing references in `mistakes.json` / `progress.json`) unchanged.

**Architecture:** Path is derived mechanically from `course.id` (which always begins with `yyyy-mm-dd-` by construction in `llm::reflexion::unique_id`). A new private helper `course_path()` centralizes the derivation. `list_courses` scans one level (`<courses_dir>/<yyyy-mm>/*.json`). A new `src/storage/migrate.rs` module handles the boot-time move; well-formed flat files go to `yyyy-mm/`, malformed-id files (and same-name conflicts) go to `courses/_legacy/`. Validation gains a `yyyy-mm-dd-` prefix check.

**Tech Stack:** Rust 1.x, `std::fs`, `chrono` (already used), `serde_json` (already used). **No new crate dependencies** — the prefix check uses a small byte helper rather than `regex`.

**Spec:** `docs/superpowers/specs/2026-05-06-courses-yyyy-mm-subdir-design.md`

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src/storage/course.rs` | Modify | Add `course_path` helper, `has_yyyy_mm_dd_prefix` byte check, `IdMissingDatePrefix` validation, `StorageError::InvalidId`; route `load_course` / `save_course` / `delete_course` through `course_path`; update `list_courses` to scan `<dir>/<yyyy-mm>/*.json` |
| `src/storage/migrate.rs` | Create | `migrate_courses_to_yyyy_mm(courses_dir, &mut Vec<String>) -> Result<(), StorageError>` |
| `src/storage/mod.rs` | Modify | `pub mod migrate;` |
| `src/main.rs` | Modify | Call `migrate_courses_to_yyyy_mm` before `list_courses`/`prune_orphans` |

---

## Task 1: Add `yyyy-mm-dd-` prefix validation

**Files:**
- Modify: `src/storage/course.rs` (add helper, validation rule, `ValidationError` variant)
- Test: `src/storage/course.rs` (existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Add a shared `sample_course` test fixture**

`course.rs` does not currently have a shared course fixture (its tests
inspect helper functions like `is_valid_soundmark` directly). Add this
helper to the existing `#[cfg(test)] mod tests` block at the top, before
any new test:

```rust
fn sample_course() -> Course {
    use chrono::TimeZone;
    fn drill(stage: u32, focus: Focus) -> Drill {
        Drill {
            stage,
            focus,
            chinese: "你好".into(),
            english: "hi there".into(),
            soundmark: "/haɪ ðɛər/".into(),
        }
    }
    fn sentence(order: u32) -> Sentence {
        Sentence {
            order,
            drills: vec![
                drill(1, Focus::Keywords),
                drill(2, Focus::Skeleton),
                drill(3, Focus::Full),
            ],
        }
    }
    Course {
        schema_version: SCHEMA_VERSION,
        id: "2026-05-06-sample".into(),
        title: "Sample".into(),
        description: None,
        source: Source {
            kind: SourceKind::Manual,
            url: String::new(),
            created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 6, 0, 0, 0).unwrap(),
            model: "test".into(),
        },
        sentences: vec![sentence(1), sentence(2), sentence(3), sentence(4), sentence(5)],
    }
}

#[test]
fn sample_course_passes_validate() {
    // Sanity: the fixture itself must be valid, otherwise downstream
    // tests that mutate fields would fail for the wrong reason.
    let c = sample_course();
    assert!(c.validate().is_empty(), "fixture invalid: {:?}", c.validate());
}
```

- [ ] **Step 2: Write the failing tests**

Append to the `tests` module in `src/storage/course.rs`:

```rust
#[test]
fn validate_rejects_id_without_yyyy_mm_dd_prefix() {
    let mut c = sample_course();
    c.id = "context-management-in-claude-code".into();
    let errs = c.validate();
    assert!(
        errs.iter().any(|e| matches!(e, ValidationError::IdMissingDatePrefix(_))),
        "expected IdMissingDatePrefix, got {errs:?}"
    );
}

#[test]
fn validate_accepts_id_with_yyyy_mm_dd_prefix() {
    let c = sample_course(); // fixture id is already 2026-05-06-...
    let errs = c.validate();
    assert!(
        !errs.iter().any(|e| matches!(e, ValidationError::IdMissingDatePrefix(_))),
        "unexpected IdMissingDatePrefix in {errs:?}"
    );
}

#[test]
fn has_yyyy_mm_dd_prefix_accepts_well_formed() {
    assert!(has_yyyy_mm_dd_prefix("2026-05-06-foo"));
    assert!(has_yyyy_mm_dd_prefix("0000-00-00-x"));
}

#[test]
fn has_yyyy_mm_dd_prefix_rejects_malformed() {
    assert!(!has_yyyy_mm_dd_prefix(""));
    assert!(!has_yyyy_mm_dd_prefix("2026-05-06"));        // no trailing dash
    assert!(!has_yyyy_mm_dd_prefix("2026-5-06-foo"));     // not zero-padded
    assert!(!has_yyyy_mm_dd_prefix("foo-2026-05-06-bar"));
    assert!(!has_yyyy_mm_dd_prefix("2026/05/06-foo"));
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cargo test --lib storage::course::tests::validate_rejects_id_without_yyyy_mm_dd_prefix \
           storage::course::tests::has_yyyy_mm_dd_prefix_accepts_well_formed \
           storage::course::tests::has_yyyy_mm_dd_prefix_rejects_malformed -- --nocapture
```

Expected: compile errors on `ValidationError::IdMissingDatePrefix` and `has_yyyy_mm_dd_prefix` (both not yet defined).

- [ ] **Step 4: Add the byte helper and validation variant**

Add near the bottom of `src/storage/course.rs`, next to `is_kebab_case`:

```rust
/// True iff `s` starts with `\d{4}-\d{2}-\d{2}-`.
/// Pure-std byte check (no regex dependency); ASCII digits only.
pub(crate) fn has_yyyy_mm_dd_prefix(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 11
        && b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && b[10] == b'-'
}
```

Add to the `ValidationError` enum (the existing `#[derive(Debug, PartialEq, Eq, Error)] pub enum ValidationError`):

```rust
    #[error("id is missing yyyy-mm-dd- prefix: {0:?}")]
    IdMissingDatePrefix(String),
```

In `Course::validate()`, immediately after the existing `is_kebab_case` block:

```rust
if !has_yyyy_mm_dd_prefix(&self.id) {
    errs.push(ValidationError::IdMissingDatePrefix(self.id.clone()));
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test --lib storage::course::tests -- --nocapture
```

Expected: all course tests pass (existing + fixture sanity + 4 new).

- [ ] **Step 6: Commit**

```bash
git add src/storage/course.rs
git commit -m "feat(course): validate yyyy-mm-dd- id prefix"
```

---

## Task 2: Add `course_path` helper and `StorageError::InvalidId`

**Files:**
- Modify: `src/storage/course.rs`
- Test: `src/storage/course.rs` tests module

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `src/storage/course.rs`:

```rust
#[test]
fn course_path_derives_yyyy_mm_dd_layout() {
    use std::path::PathBuf;
    let p = course_path(std::path::Path::new("/tmp/courses"), "2026-05-06-foo-bar").unwrap();
    assert_eq!(
        p,
        PathBuf::from("/tmp/courses/2026-05/06-foo-bar.json")
    );
}

#[test]
fn course_path_rejects_id_without_prefix() {
    let err = course_path(std::path::Path::new("/tmp/c"), "foo").unwrap_err();
    assert!(matches!(err, StorageError::InvalidId(_)));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib storage::course::tests::course_path_ -- --nocapture
```

Expected: compile errors on `course_path` and `StorageError::InvalidId` (both not yet defined).

- [ ] **Step 3: Add `StorageError::InvalidId` variant**

In the existing `enum StorageError`:

```rust
    #[error("invalid course id (must match yyyy-mm-dd-<slug>): {0:?}")]
    InvalidId(String),
```

- [ ] **Step 4: Add the `course_path` private helper**

Place this immediately above `pub fn list_courses` in `src/storage/course.rs`:

```rust
/// Derives the on-disk path for a course id of shape `yyyy-mm-dd-<rest>`.
///
/// Returns `StorageError::InvalidId` if the id does not begin with that
/// prefix; this guards the byte-slice indices below so the function never
/// panics on a malformed id supplied by an external caller.
fn course_path(courses_dir: &std::path::Path, id: &str) -> Result<std::path::PathBuf, StorageError> {
    if !has_yyyy_mm_dd_prefix(id) {
        return Err(StorageError::InvalidId(id.to_string()));
    }
    let yyyy_mm = &id[0..7];                  // "2026-05"
    let file = format!("{}.json", &id[8..]);  // "06-foo-bar.json"
    Ok(courses_dir.join(yyyy_mm).join(file))
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test --lib storage::course::tests -- --nocapture
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/storage/course.rs
git commit -m "feat(course): add course_path helper and InvalidId error"
```

---

## Task 3: Route `load_course` / `save_course` / `delete_course` through `course_path`

**Files:**
- Modify: `src/storage/course.rs` lines around 455–490

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `src/storage/course.rs`:

```rust
#[test]
fn save_then_load_roundtrip_uses_yyyy_mm_subdir() {
    use tempfile::tempdir;
    let dir = tempdir().unwrap();
    let mut c = sample_course();
    c.id = "2026-05-06-roundtrip".into();
    save_course(dir.path(), &c).unwrap();

    let written = dir.path().join("2026-05").join("06-roundtrip.json");
    assert!(written.exists(), "expected file at {written:?}");

    let back = load_course(dir.path(), "2026-05-06-roundtrip").unwrap();
    assert_eq!(back.id, "2026-05-06-roundtrip");
}

#[test]
fn delete_course_removes_yyyy_mm_file() {
    use tempfile::tempdir;
    let dir = tempdir().unwrap();
    let mut c = sample_course();
    c.id = "2026-05-06-todel".into();
    save_course(dir.path(), &c).unwrap();
    delete_course(dir.path(), "2026-05-06-todel").unwrap();
    assert!(!dir.path().join("2026-05").join("06-todel.json").exists());
}

#[test]
fn load_course_returns_not_found_for_missing_id() {
    use tempfile::tempdir;
    let dir = tempdir().unwrap();
    let err = load_course(dir.path(), "2026-05-06-missing").unwrap_err();
    assert!(matches!(err, StorageError::NotFound(_)));
}
```

If `tempfile` is not yet a dev-dependency in `Cargo.toml`, check whether it is: `grep -n tempfile Cargo.toml`. If absent, run `cargo add --dev tempfile@3` before the test compiles. (`storage/failed.rs` already uses `tempfile::tempdir`, so it should already be present.)

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib storage::course::tests::save_then_load_roundtrip_uses_yyyy_mm_subdir \
           storage::course::tests::delete_course_removes_yyyy_mm_file \
           storage::course::tests::load_course_returns_not_found_for_missing_id -- --nocapture
```

Expected: tests compile but fail on path assertions (file not at the expected `2026-05/` subdir; old code writes to `<dir>/2026-05-06-roundtrip.json`).

- [ ] **Step 3: Update `load_course`**

Replace the body of `load_course` in `src/storage/course.rs:455`:

```rust
pub fn load_course(courses_dir: &std::path::Path, id: &str) -> Result<Course, StorageError> {
    let path = course_path(courses_dir, id)?;
    let bytes = std::fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StorageError::NotFound(id.into())
        } else {
            StorageError::Io(e)
        }
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}
```

- [ ] **Step 4: Update `save_course`**

Replace the body of `save_course`:

```rust
pub fn save_course(courses_dir: &std::path::Path, course: &Course) -> Result<(), StorageError> {
    debug_assert!(
        is_kebab_case(&course.id),
        "save_course called with non-kebab-case id: {:?}",
        course.id
    );
    let path = course_path(courses_dir, &course.id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(course)?;
    write_atomic(&path, &bytes)?;
    Ok(())
}
```

- [ ] **Step 5: Update `delete_course`**

Replace the body of `delete_course`:

```rust
pub fn delete_course(courses_dir: &std::path::Path, id: &str) -> Result<(), StorageError> {
    let path = course_path(courses_dir, id)?;
    std::fs::remove_file(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StorageError::NotFound(id.into())
        } else {
            StorageError::Io(e)
        }
    })?;
    Ok(())
}
```

- [ ] **Step 6: Run tests to verify they pass**

```bash
cargo test --lib storage::course::tests -- --nocapture
```

Expected: new tests pass; existing tests in this module also pass (they all use `tempdir` and round-trip through these functions, so they'll automatically observe the new layout).

- [ ] **Step 7: Commit**

```bash
git add src/storage/course.rs
git commit -m "feat(course): write and read courses under yyyy-mm/ subdirs"
```

---

## Task 4: Make `list_courses` scan `<dir>/<yyyy-mm>/*.json` only

**Files:**
- Modify: `src/storage/course.rs:423` (`list_courses`)

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `src/storage/course.rs`:

```rust
#[test]
fn list_courses_scans_yyyy_mm_subdirs_and_skips_others() {
    use tempfile::tempdir;
    let dir = tempdir().unwrap();

    // Well-formed file in yyyy-mm/
    let mut c1 = sample_course();
    c1.id = "2026-05-06-alpha".into();
    save_course(dir.path(), &c1).unwrap();

    // Well-formed file in a different month
    let mut c2 = sample_course();
    c2.id = "2026-04-01-beta".into();
    save_course(dir.path(), &c2).unwrap();

    // Stray root-level json (should be ignored)
    std::fs::write(dir.path().join("stray.json"), b"{}").unwrap();

    // _legacy/ dir with a json (should be ignored)
    std::fs::create_dir_all(dir.path().join("_legacy")).unwrap();
    std::fs::write(dir.path().join("_legacy/old.json"), b"{}").unwrap();

    // Non-yyyy-mm subdir (should be ignored)
    std::fs::create_dir_all(dir.path().join("tmp")).unwrap();
    std::fs::write(dir.path().join("tmp/junk.json"), b"{}").unwrap();

    let metas = list_courses(dir.path()).unwrap();
    let ids: Vec<_> = metas.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids.len(), 2, "expected 2 ids, got {ids:?}");
    assert!(ids.contains(&"2026-05-06-alpha"));
    assert!(ids.contains(&"2026-04-01-beta"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --lib storage::course::tests::list_courses_scans_yyyy_mm_subdirs_and_skips_others -- --nocapture
```

Expected: fails — current `list_courses` reads the root directory and finds nothing under `<dir>/2026-05/` etc.

- [ ] **Step 3: Rewrite `list_courses`**

Replace the body of `list_courses`:

```rust
pub fn list_courses(courses_dir: &std::path::Path) -> Result<Vec<CourseMeta>, StorageError> {
    let mut out = Vec::new();
    if !courses_dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(courses_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_yyyy_mm_dirname(name) {
            continue; // skips _legacy/, tmp/, etc.
        }
        for sub in std::fs::read_dir(&path)? {
            let sub = sub?;
            let sub_path = sub.path();
            if sub_path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            // Skip unreadable or corrupt files silently — one bad file must
            // not break the whole list page.
            let Ok(bytes) = std::fs::read(&sub_path) else {
                continue;
            };
            let Ok(course) = serde_json::from_slice::<Course>(&bytes) else {
                continue;
            };
            // Defensive: id field must match its on-disk location.
            // Mismatches indicate a manually-placed file; skip with a log.
            let expected_stem = format!(
                "{}-{}",
                name,
                course.id.get(8..).unwrap_or("")
            );
            if expected_stem != course.id {
                tracing::warn!(
                    "list_courses: id/path mismatch (path={sub_path:?}, id={:?}); skipped",
                    course.id
                );
                continue;
            }
            let total_drills = course.sentences.iter().map(|s| s.drills.len()).sum();
            out.push(CourseMeta {
                id: course.id,
                title: course.title,
                created_at: course.source.created_at,
                total_sentences: course.sentences.len(),
                total_drills,
            });
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(out)
}

/// True iff `s` matches `\d{4}-\d{2}` (e.g. "2026-05").
fn is_yyyy_mm_dirname(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7
        && b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib storage::course::tests -- --nocapture
```

Expected: all course tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/storage/course.rs
git commit -m "feat(course): list_courses scans yyyy-mm subdirs only"
```

---

## Task 5: Create `migrate_courses_to_yyyy_mm`

**Files:**
- Create: `src/storage/migrate.rs`
- Modify: `src/storage/mod.rs` (add `pub mod migrate;`)
- Test: `src/storage/migrate.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Create `src/storage/migrate.rs` with failing tests**

Write the file with the test module first (no implementation yet, just stubs that won't compile):

```rust
//! One-shot startup migration moving flat `courses/<id>.json` files into
//! `courses/<yyyy-mm>/<dd-rest>.json`. Files with a malformed id, or whose
//! target path already exists, are moved to `courses/_legacy/` and a
//! boot warning is emitted.

use std::path::Path;

use crate::storage::course::{has_yyyy_mm_dd_prefix, Course, StorageError};

/// Result counters returned for inspection / boot warnings.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrateReport {
    pub moved: u32,
    pub legacy_malformed: u32,
    pub legacy_conflict: u32,
}

/// Walks direct children of `courses_dir`. For each `*.json` file:
/// - if its `course.id` parses and matches `yyyy-mm-dd-...`, rename to
///   `<courses_dir>/<yyyy-mm>/<dd-rest>.json` (mkdir -p as needed);
/// - otherwise (or if target already exists) rename to
///   `<courses_dir>/_legacy/<basename>`.
///
/// Pushes user-facing strings into `boot_warnings` when files end up in
/// `_legacy/`. Returns an error on any IO failure (caller aborts startup).
pub fn migrate_courses_to_yyyy_mm(
    courses_dir: &Path,
    boot_warnings: &mut Vec<String>,
) -> Result<MigrateReport, StorageError> {
    todo!("implement in step 3")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_course_json(path: &Path, id: &str) {
        // Minimal valid Course JSON — schema v2, 5 sentences × 3 drills,
        // last drill of each sentence is "full".
        let body = format!(
            r#"{{
  "schemaVersion": 2,
  "id": "{id}",
  "title": "T",
  "source": {{"type":"manual","url":"","createdAt":"2026-05-06T00:00:00Z","model":"m"}},
  "sentences": [
    {{"order":1,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}}
    ]}},
    {{"order":2,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}}
    ]}},
    {{"order":3,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}}
    ]}},
    {{"order":4,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}}
    ]}},
    {{"order":5,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ ðɛər/"}}
    ]}}
  ]
}}"#
        );
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn moves_well_formed_flat_file_to_yyyy_mm() {
        let d = tempdir().unwrap();
        let id = "2026-05-06-alpha";
        write_course_json(&d.path().join(format!("{id}.json")), id);

        let mut warnings = Vec::new();
        let report = migrate_courses_to_yyyy_mm(d.path(), &mut warnings).unwrap();

        assert!(d.path().join("2026-05/06-alpha.json").exists());
        assert!(!d.path().join(format!("{id}.json")).exists());
        assert_eq!(report.moved, 1);
        assert_eq!(report.legacy_malformed, 0);
        assert_eq!(report.legacy_conflict, 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn malformed_id_goes_to_legacy_with_warning() {
        let d = tempdir().unwrap();
        write_course_json(&d.path().join("not-a-date-foo.json"), "not-a-date-foo");

        let mut warnings = Vec::new();
        let report = migrate_courses_to_yyyy_mm(d.path(), &mut warnings).unwrap();

        assert!(d.path().join("_legacy/not-a-date-foo.json").exists());
        assert!(!d.path().join("not-a-date-foo.json").exists());
        assert_eq!(report.legacy_malformed, 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("malformed"));
    }

    #[test]
    fn unparseable_json_goes_to_legacy_with_warning() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("garbage.json"), b"not json").unwrap();

        let mut warnings = Vec::new();
        let report = migrate_courses_to_yyyy_mm(d.path(), &mut warnings).unwrap();

        assert!(d.path().join("_legacy/garbage.json").exists());
        assert_eq!(report.legacy_malformed, 1);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn target_conflict_moves_source_to_legacy() {
        let d = tempdir().unwrap();
        let id = "2026-05-06-dup";
        // Pre-create the target.
        std::fs::create_dir_all(d.path().join("2026-05")).unwrap();
        std::fs::write(d.path().join("2026-05/06-dup.json"), b"existing").unwrap();
        // And a flat source with the same id.
        write_course_json(&d.path().join(format!("{id}.json")), id);

        let mut warnings = Vec::new();
        let report = migrate_courses_to_yyyy_mm(d.path(), &mut warnings).unwrap();

        assert_eq!(
            std::fs::read(d.path().join("2026-05/06-dup.json")).unwrap(),
            b"existing"
        );
        assert!(d.path().join(format!("_legacy/{id}.json")).exists());
        assert_eq!(report.legacy_conflict, 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("conflict"));
    }

    #[test]
    fn idempotent_when_already_migrated() {
        let d = tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("2026-05")).unwrap();
        std::fs::write(d.path().join("2026-05/06-alpha.json"), b"{}").unwrap();

        let mut warnings = Vec::new();
        let report = migrate_courses_to_yyyy_mm(d.path(), &mut warnings).unwrap();

        assert_eq!(report, MigrateReport::default());
        assert!(warnings.is_empty());
        assert!(d.path().join("2026-05/06-alpha.json").exists());
    }

    #[test]
    fn nonexistent_courses_dir_is_noop() {
        let d = tempdir().unwrap();
        let missing = d.path().join("does-not-exist");
        let mut warnings = Vec::new();
        let report = migrate_courses_to_yyyy_mm(&missing, &mut warnings).unwrap();
        assert_eq!(report, MigrateReport::default());
        assert!(warnings.is_empty());
    }
}
```

Add `pub mod migrate;` to `src/storage/mod.rs`:

```rust
pub mod atomic;
pub mod course;
pub mod failed;
pub mod migrate;
pub mod mistakes;
pub mod paths;
pub mod progress;
```

Mark `has_yyyy_mm_dd_prefix` as `pub(crate)` in `course.rs` so `migrate.rs` can use it (already done in Task 1 if you wrote `pub(crate)` — verify).

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib storage::migrate::tests -- --nocapture
```

Expected: tests panic on `todo!()` from the unimplemented function.

- [ ] **Step 3: Implement `migrate_courses_to_yyyy_mm`**

Replace the `todo!()` body in `src/storage/migrate.rs`:

```rust
pub fn migrate_courses_to_yyyy_mm(
    courses_dir: &Path,
    boot_warnings: &mut Vec<String>,
) -> Result<MigrateReport, StorageError> {
    let mut report = MigrateReport::default();

    if !courses_dir.exists() {
        return Ok(report);
    }

    let legacy_dir = courses_dir.join("_legacy");

    for entry in std::fs::read_dir(courses_dir)? {
        let entry = entry?;
        let src = entry.path();
        // Only flat root-level *.json files are candidates.
        if !src.is_file() {
            continue;
        }
        if src.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let basename = match src.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Decide: well-formed → move to yyyy-mm/, else → _legacy/.
        let target = match read_id(&src) {
            Some(id) if has_yyyy_mm_dd_prefix(&id) => {
                // <yyyy-mm>/<dd-rest>.json
                let yyyy_mm = id[0..7].to_string();
                let file = format!("{}.json", &id[8..]);
                Some(courses_dir.join(yyyy_mm).join(file))
            }
            _ => None,
        };

        match target {
            Some(t) if !t.exists() => {
                if let Some(parent) = t.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(&src, &t)?;
                report.moved += 1;
            }
            Some(_t) => {
                // Conflict: target exists. Move source to _legacy/.
                std::fs::create_dir_all(&legacy_dir)?;
                std::fs::rename(&src, legacy_dir.join(&basename))?;
                report.legacy_conflict += 1;
            }
            None => {
                // Malformed (couldn't parse or id lacks date prefix).
                std::fs::create_dir_all(&legacy_dir)?;
                std::fs::rename(&src, legacy_dir.join(&basename))?;
                report.legacy_malformed += 1;
            }
        }
    }

    if report.legacy_malformed > 0 {
        boot_warnings.push(format!(
            "Moved {} malformed course file(s) to _legacy/ — please review",
            report.legacy_malformed
        ));
    }
    if report.legacy_conflict > 0 {
        boot_warnings.push(format!(
            "Moved {} course file(s) to _legacy/ due to path conflicts — please review",
            report.legacy_conflict
        ));
    }
    Ok(report)
}

fn read_id(path: &Path) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct IdOnly {
        id: String,
    }
    let bytes = std::fs::read(path).ok()?;
    let parsed: IdOnly = serde_json::from_slice(&bytes).ok()?;
    Some(parsed.id)
}
```

Note: `Course` import in the `use` line at the top of the file is no longer needed once `read_id` uses an inline `IdOnly` shim. Drop the `Course` from the `use` to avoid an unused-import warning:

```rust
use crate::storage::course::{has_yyyy_mm_dd_prefix, StorageError};
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib storage::migrate::tests -- --nocapture
```

Expected: all 6 tests pass.

- [ ] **Step 5: Run full test suite**

```bash
cargo test --lib
```

Expected: everything green.

- [ ] **Step 6: Commit**

```bash
git add src/storage/migrate.rs src/storage/mod.rs src/storage/course.rs
git commit -m "feat(course): add startup migration to yyyy-mm/ layout"
```

---

## Task 6: Wire migration into `main.rs`

**Files:**
- Modify: `src/main.rs` around the boot sequence (just after `boot_warnings` is initialized at line 65, before `list_courses` at line 97)

- [ ] **Step 1: Read the surrounding boot code**

```bash
sed -n '60,110p' src/main.rs
```

Confirm `boot_warnings: Vec<String>` is in scope at the chosen insertion point.

- [ ] **Step 2: Insert the migration call**

After `let mut boot_warnings: Vec<String> = Vec::new();` and any other warning-producing setup that does not touch courses, add (before the `list_courses` call around line 97):

```rust
// One-shot migration: flat courses/*.json → courses/yyyy-mm/dd-rest.json.
// Idempotent on subsequent runs. Fatal on IO error: a partial migration
// leaves harder-to-reason-about state than a clean abort.
inkworm::storage::migrate::migrate_courses_to_yyyy_mm(
    &paths.courses_dir,
    &mut boot_warnings,
)?;
```

- [ ] **Step 3: Build the binary**

```bash
cargo build
```

Expected: clean build, no warnings.

- [ ] **Step 4: Verify with a real-data smoke test**

Use a temp config dir to avoid touching real user state:

```bash
mkdir -p /tmp/inkworm-smoke/courses
cp ~/.config/inkworm/courses/2026-05-06-context-management-in-claude-code.json \
   /tmp/inkworm-smoke/courses/
# Quick run that exits early — we only care that startup migration ran.
INKWORM_CONFIG_DIR=/tmp/inkworm-smoke cargo run -- --version
ls /tmp/inkworm-smoke/courses/
ls /tmp/inkworm-smoke/courses/2026-05/
```

Expected:
- `courses/2026-05-06-context-management-in-claude-code.json` is gone
- `courses/2026-05/06-context-management-in-claude-code.json` exists
- No `_legacy/` directory

If `INKWORM_CONFIG_DIR` is not the env var name your project uses, run `grep -n CONFIG_DIR src/storage/paths.rs` and substitute.

If the smoke test is hard to wire (e.g. no env-var override), skip it and rely on the unit tests in Task 5; note this in the commit body.

- [ ] **Step 5: Run full test suite**

```bash
cargo test
```

Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(boot): run courses yyyy-mm migration at startup"
```

---

## Task 7: Manual end-to-end verification on real config dir

This task does not write code — it confirms the integrated change works on real data before merging.

- [ ] **Step 1: Snapshot real data**

```bash
cp -a ~/.config/inkworm/courses ~/.config/inkworm/courses.bak.before-migrate
```

- [ ] **Step 2: Run inkworm against real config dir**

```bash
cargo run --release -- --version
```

(any short-lived invocation that triggers boot is fine.)

- [ ] **Step 3: Inspect the result**

```bash
ls ~/.config/inkworm/courses/
ls ~/.config/inkworm/courses/2026-05/
```

Expected:
- root contains only `2026-05/` (no `*.json` files at the root)
- `2026-05/` contains `06-context-management-in-claude-code.json`
- no `_legacy/` directory (the existing file is well-formed)

- [ ] **Step 4: Sanity-check `mistakes.json` still resolves**

```bash
cat ~/.config/inkworm/mistakes.json
```

The `wrongStreaks` key `2026-05-06-context-management-in-claude-code|1|2` should still be present. Then launch inkworm interactively (`cargo run --release`) and verify the course shows up in `/list` and that loading it succeeds.

- [ ] **Step 5: If everything is good, drop the snapshot**

```bash
rm -rf ~/.config/inkworm/courses.bak.before-migrate
```

If anything looks wrong, restore: `rm -rf ~/.config/inkworm/courses && mv ~/.config/inkworm/courses.bak.before-migrate ~/.config/inkworm/courses`.

- [ ] **Step 6: Final commit (only if a docstring or comment tweak fell out of verification)**

If verification surfaced a doc nit, fix it and:

```bash
git add <file>
git commit -m "docs(course): clarify <whatever>"
```

Otherwise skip — no need for an empty commit.

---

## Self-Review Checklist (engineer to run before claiming done)

- [ ] All unit tests pass: `cargo test`
- [ ] No new warnings: `cargo build` produces zero warnings
- [ ] `cargo fmt --check` clean (or `rustfmt --check src/storage/course.rs src/storage/migrate.rs src/main.rs` per project's known fmt-check quirk with file args)
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] Real-data run (Task 7) verified the migration moved the file and `mistakes.json` reference still resolves
- [ ] `git log --oneline` shows ≤7 small commits, each conventional, each independently buildable
