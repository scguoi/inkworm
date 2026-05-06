//! Course schema (v2): one article → N sentences → 3–5 progressive drills each.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::storage::atomic::write_atomic;

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Course {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    pub source: Source,
    pub sentences: Vec<Sentence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    #[serde(rename = "type")]
    pub kind: SourceKind,
    #[serde(default)]
    pub url: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Article,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sentence {
    pub order: u32,
    pub drills: Vec<Drill>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drill {
    pub stage: u32,
    pub focus: Focus,
    pub chinese: String,
    pub english: String,
    pub soundmark: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Focus {
    Keywords,
    Skeleton,
    Clause,
    Full,
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("schemaVersion must be {expected}, got {actual}")]
    WrongSchemaVersion { expected: u32, actual: u32 },
    #[error("id is empty or not kebab-case: {0:?}")]
    InvalidId(String),
    #[error("id is missing yyyy-mm-dd- prefix: {0:?}")]
    IdMissingDatePrefix(String),
    #[error("title length must be 1..=100, got {0}")]
    TitleLength(usize),
    #[error("description length must be ≤300, got {0}")]
    DescriptionTooLong(usize),
    #[error("sentences length must be 5..=20, got {0}")]
    SentencesCount(usize),
    #[error("sentences[{index}].order must be {expected}, got {actual}")]
    SentenceOrder {
        index: usize,
        expected: u32,
        actual: u32,
    },
    #[error("sentences[{sentence}].drills length must be 3..=5, got {count}")]
    DrillsCount { sentence: usize, count: usize },
    #[error("sentences[{sentence}].drills[{drill}].stage must be {expected}, got {actual}")]
    DrillStage {
        sentence: usize,
        drill: usize,
        expected: u32,
        actual: u32,
    },
    #[error("sentences[{sentence}] last drill focus must be \"full\"")]
    LastDrillNotFull { sentence: usize },
    #[error("sentences[{sentence}].drills[{drill}].chinese length must be 1..=200, got {len}")]
    ChineseLength {
        sentence: usize,
        drill: usize,
        len: usize,
    },
    #[error(
        "sentences[{sentence}].drills[{drill}].chinese must contain Chinese characters (Hanzi), got {value:?}"
    )]
    ChineseNotInChinese {
        sentence: usize,
        drill: usize,
        value: String,
    },
    #[error(
        "sentences[{sentence}].drills[{drill}].english word count must be 1..=50, got {words}"
    )]
    EnglishWordCount {
        sentence: usize,
        drill: usize,
        words: usize,
    },
    #[error("sentences[{sentence}].drills[{drill}].soundmark format invalid: {value:?}")]
    SoundmarkFormat {
        sentence: usize,
        drill: usize,
        value: String,
    },
    #[error(
        "sentences[{sentence}].drills[{drill}].soundmark must not be empty for focus={focus:?}"
    )]
    SoundmarkMissing {
        sentence: usize,
        drill: usize,
        focus: Focus,
    },
    #[error(
        "sentences[{sentence}].drills[{drill}].english contains IPA symbols (belongs in soundmark): {value:?}"
    )]
    EnglishContainsIpa {
        sentence: usize,
        drill: usize,
        value: String,
    },
}

impl Course {
    /// Returns `Vec<ValidationError>`, empty if valid. Collects ALL violations,
    /// does not short-circuit on first.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errs = Vec::new();

        if self.schema_version != SCHEMA_VERSION {
            errs.push(ValidationError::WrongSchemaVersion {
                expected: SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }
        if !is_kebab_case(&self.id) {
            errs.push(ValidationError::InvalidId(self.id.clone()));
        }
        if !has_yyyy_mm_dd_prefix(&self.id) {
            errs.push(ValidationError::IdMissingDatePrefix(self.id.clone()));
        }
        if self.title.is_empty() || self.title.chars().count() > 100 {
            errs.push(ValidationError::TitleLength(self.title.chars().count()));
        }
        if let Some(d) = &self.description {
            if d.chars().count() > 300 {
                errs.push(ValidationError::DescriptionTooLong(d.chars().count()));
            }
        }
        let n = self.sentences.len();
        if !(5..=20).contains(&n) {
            errs.push(ValidationError::SentencesCount(n));
        }
        for (i, s) in self.sentences.iter().enumerate() {
            let expected_order = (i as u32) + 1;
            if s.order != expected_order {
                errs.push(ValidationError::SentenceOrder {
                    index: i,
                    expected: expected_order,
                    actual: s.order,
                });
            }
            let dn = s.drills.len();
            if !(3..=5).contains(&dn) {
                errs.push(ValidationError::DrillsCount {
                    sentence: i,
                    count: dn,
                });
            }
            for (j, d) in s.drills.iter().enumerate() {
                let expected_stage = (j as u32) + 1;
                if d.stage != expected_stage {
                    errs.push(ValidationError::DrillStage {
                        sentence: i,
                        drill: j,
                        expected: expected_stage,
                        actual: d.stage,
                    });
                }
                let clen = d.chinese.chars().count();
                if !(1..=200).contains(&clen) {
                    errs.push(ValidationError::ChineseLength {
                        sentence: i,
                        drill: j,
                        len: clen,
                    });
                }
                if !contains_hanzi(&d.chinese) {
                    errs.push(ValidationError::ChineseNotInChinese {
                        sentence: i,
                        drill: j,
                        value: d.chinese.clone(),
                    });
                }
                let words = d.english.split_whitespace().count();
                if !(1..=50).contains(&words) {
                    errs.push(ValidationError::EnglishWordCount {
                        sentence: i,
                        drill: j,
                        words,
                    });
                }
                if contains_ipa_marker(&d.english) {
                    errs.push(ValidationError::EnglishContainsIpa {
                        sentence: i,
                        drill: j,
                        value: d.english.clone(),
                    });
                }
                if !is_valid_soundmark(&d.soundmark) {
                    errs.push(ValidationError::SoundmarkFormat {
                        sentence: i,
                        drill: j,
                        value: d.soundmark.clone(),
                    });
                }
                // All stages must have soundmark
                if d.soundmark.is_empty() {
                    errs.push(ValidationError::SoundmarkMissing {
                        sentence: i,
                        drill: j,
                        focus: d.focus,
                    });
                }
            }
            if let Some(last) = s.drills.last() {
                if last.focus != Focus::Full {
                    errs.push(ValidationError::LastDrillNotFull { sentence: i });
                }
            }
        }

        errs
    }
}

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

fn is_kebab_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

/// IPA symbols that must never appear in a drill's `english` field.
/// Stress/length markers and vowels/consonants absent from ordinary
/// English orthography — presence signals the LLM leaked soundmark
/// content into the english field.
fn contains_ipa_marker(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(
            c,
            'ˈ' | 'ˌ'
                | 'ː'
                | 'ə'
                | 'ɒ'
                | 'ɜ'
                | 'ʌ'
                | 'ɪ'
                | 'ʊ'
                | 'ɛ'
                | 'ɔ'
                | 'ɑ'
                | 'æ'
                | 'θ'
                | 'ð'
                | 'ʃ'
                | 'ʒ'
                | 'ŋ'
        )
    })
}

/// Returns true if `s` contains at least one CJK Unified Ideograph
/// (incl. Extension A). Used to catch LLM outputs that put English in
/// the `chinese` field.
fn contains_hanzi(s: &str) -> bool {
    s.chars()
        .any(|c| ('\u{3400}'..='\u{4DBF}').contains(&c) || ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

fn is_valid_soundmark(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    // Must match (/[^/]+/\s*)+, where each /...../ contains no whitespace
    // or '.' (one continuous IPA string per word — syllables separated by
    // stress marks ˈ ˌ, not periods or spaces).
    let mut chars = s.chars().peekable();
    while chars.peek().is_some() {
        if chars.next() != Some('/') {
            return false;
        }
        let mut inner = String::new();
        loop {
            match chars.next() {
                Some('/') => break,
                Some(c) if c.is_whitespace() || c == '.' => return false,
                Some(c) => inner.push(c),
                None => return false,
            }
        }
        if inner.is_empty() {
            return false;
        }
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_course() -> Course {
        use chrono::TimeZone;
        fn drill(stage: u32, focus: Focus) -> Drill {
            Drill {
                stage,
                focus,
                chinese: "你好".into(),
                english: "hi there".into(),
                soundmark: "/haɪ/ /ðɛər/".into(),
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
            sentences: vec![
                sentence(1),
                sentence(2),
                sentence(3),
                sentence(4),
                sentence(5),
            ],
        }
    }

    #[test]
    fn sample_course_passes_validate() {
        let c = sample_course();
        assert!(
            c.validate().is_empty(),
            "fixture invalid: {:?}",
            c.validate()
        );
    }

    #[test]
    fn validate_rejects_id_without_yyyy_mm_dd_prefix() {
        let mut c = sample_course();
        c.id = "context-management-in-claude-code".into();
        let errs = c.validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::IdMissingDatePrefix(_))),
            "expected IdMissingDatePrefix, got {errs:?}"
        );
    }

    #[test]
    fn validate_accepts_id_with_yyyy_mm_dd_prefix() {
        let c = sample_course();
        let errs = c.validate();
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, ValidationError::IdMissingDatePrefix(_))),
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
        assert!(!has_yyyy_mm_dd_prefix("2026-05-06"));
        assert!(!has_yyyy_mm_dd_prefix("2026-5-06-foo"));
        assert!(!has_yyyy_mm_dd_prefix("foo-2026-05-06-bar"));
        assert!(!has_yyyy_mm_dd_prefix("2026/05/06-foo"));
    }

    #[test]
    fn course_path_derives_yyyy_mm_dd_layout() {
        use std::path::PathBuf;
        let p = course_path(std::path::Path::new("/tmp/courses"), "2026-05-06-foo-bar").unwrap();
        assert_eq!(p, PathBuf::from("/tmp/courses/2026-05/06-foo-bar.json"));
    }

    #[test]
    fn course_path_rejects_id_without_prefix() {
        let err = course_path(std::path::Path::new("/tmp/c"), "foo").unwrap_err();
        assert!(matches!(err, StorageError::InvalidId(_)));
    }

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
}

#[cfg(test)]
mod hanzi_tests {
    use super::contains_hanzi;

    #[test]
    fn ascii_only_is_not_chinese() {
        assert!(!contains_hanzi("This separates toy demos"));
    }

    #[test]
    fn pure_hanzi_is_chinese() {
        assert!(contains_hanzi("代理是涌现的行为"));
    }

    #[test]
    fn mixed_hanzi_and_ascii_is_chinese() {
        assert!(contains_hanzi("LLM 驱动的代理"));
    }

    #[test]
    fn punctuation_only_is_not_chinese() {
        assert!(!contains_hanzi("，。！？"));
    }

    #[test]
    fn empty_is_not_chinese() {
        assert!(!contains_hanzi(""));
    }
}

#[cfg(test)]
mod soundmark_tests {
    use super::is_valid_soundmark;

    #[test]
    fn empty_is_valid() {
        assert!(is_valid_soundmark(""));
    }

    #[test]
    fn per_word_slashes_are_valid() {
        assert!(is_valid_soundmark("/aɪ/ /θɪŋk/ /əˈbaʊt/"));
    }

    #[test]
    fn no_slashes_is_invalid() {
        assert!(!is_valid_soundmark("aɪ θɪŋk"));
    }

    #[test]
    fn unterminated_slash_is_invalid() {
        assert!(!is_valid_soundmark("/aɪ"));
    }

    #[test]
    fn empty_slash_group_is_invalid() {
        assert!(!is_valid_soundmark("//"));
    }

    #[test]
    fn whitespace_inside_slashes_is_invalid() {
        assert!(!is_valid_soundmark("/ðə ˈeɪɡənt/"));
    }

    #[test]
    fn period_inside_slashes_is_invalid() {
        assert!(!is_valid_soundmark("/ˈeɪ.ɡənt/"));
    }

    #[test]
    fn stress_marks_inside_slashes_are_valid() {
        assert!(is_valid_soundmark("/ˈeɪɡənt/ /ˌɪntərˈækts/"));
    }
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("course not found: {0}")]
    NotFound(String),
    #[error("invalid course id (must match yyyy-mm-dd-<slug>): {0:?}")]
    InvalidId(String),
}

#[derive(Debug, Clone)]
pub struct CourseMeta {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub total_sentences: usize,
    pub total_drills: usize,
}

/// Derives the on-disk path for a course id of shape `yyyy-mm-dd-<rest>`.
///
/// Returns `StorageError::InvalidId` if the id does not begin with that
/// prefix; this guards the byte-slice indices below so the function never
/// panics on a malformed id supplied by an external caller.
fn course_path(
    courses_dir: &std::path::Path,
    id: &str,
) -> Result<std::path::PathBuf, StorageError> {
    if !has_yyyy_mm_dd_prefix(id) {
        return Err(StorageError::InvalidId(id.to_string()));
    }
    let yyyy_mm = &id[0..7]; // "2026-05"
    let file = format!("{}.json", &id[8..]); // "06-foo-bar.json"
    Ok(courses_dir.join(yyyy_mm).join(file))
}

pub fn list_courses(courses_dir: &std::path::Path) -> Result<Vec<CourseMeta>, StorageError> {
    let mut out = Vec::new();
    if !courses_dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(courses_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // Skip unreadable or corrupt files silently — one bad file must not
        // break the whole list page.
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(course) = serde_json::from_slice::<Course>(&bytes) else {
            continue;
        };
        let total_drills = course.sentences.iter().map(|s| s.drills.len()).sum();
        out.push(CourseMeta {
            id: course.id,
            title: course.title,
            created_at: course.source.created_at,
            total_sentences: course.sentences.len(),
            total_drills,
        });
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(out)
}

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

pub fn delete_course(courses_dir: &std::path::Path, id: &str) -> Result<(), StorageError> {
    let path = course_path(courses_dir, id)?;
    std::fs::remove_file(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StorageError::NotFound(id.into())
        } else {
            StorageError::Io(e)
        }
    })
}
