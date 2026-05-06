//! One-shot startup migration moving flat `courses/<id>.json` files into
//! `courses/<yyyy-mm>/<dd-rest>.json`. Files with a malformed id, or whose
//! target path already exists, are moved to `courses/_legacy/` and a
//! boot warning is emitted.

use std::path::Path;

use crate::storage::course::{has_yyyy_mm_dd_prefix, StorageError};

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
    let mut report = MigrateReport::default();

    if !courses_dir.exists() {
        return Ok(report);
    }

    let legacy_dir = courses_dir.join("_legacy");

    for entry in std::fs::read_dir(courses_dir)? {
        let entry = entry?;
        let src = entry.path();
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

        let target = match read_id(&src) {
            Some(id) if has_yyyy_mm_dd_prefix(&id) => {
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
                std::fs::create_dir_all(&legacy_dir)?;
                std::fs::rename(&src, legacy_dir.join(&basename))?;
                report.legacy_conflict += 1;
            }
            None => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_course_json(path: &Path, id: &str) {
        let body = format!(
            r#"{{
  "schemaVersion": 2,
  "id": "{id}",
  "title": "T",
  "source": {{"type":"manual","url":"","createdAt":"2026-05-06T00:00:00Z","model":"m"}},
  "sentences": [
    {{"order":1,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}}
    ]}},
    {{"order":2,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}}
    ]}},
    {{"order":3,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}}
    ]}},
    {{"order":4,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}}
    ]}},
    {{"order":5,"drills":[
      {{"stage":1,"focus":"keywords","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":2,"focus":"skeleton","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}},
      {{"stage":3,"focus":"full","chinese":"你好","english":"hi there","soundmark":"/haɪ/ /ðɛər/"}}
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
        std::fs::create_dir_all(d.path().join("2026-05")).unwrap();
        std::fs::write(d.path().join("2026-05/06-dup.json"), b"existing").unwrap();
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
