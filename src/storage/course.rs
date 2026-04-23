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
                let words = d.english.split_whitespace().count();
                if !(1..=50).contains(&words) {
                    errs.push(ValidationError::EnglishWordCount {
                        sentence: i,
                        drill: j,
                        words,
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

fn is_valid_soundmark(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    // Must match (/[^/]+/\s*)+
    let mut chars = s.chars().peekable();
    while chars.peek().is_some() {
        if chars.next() != Some('/') {
            return false;
        }
        let mut inner = String::new();
        loop {
            match chars.next() {
                Some('/') => break,
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

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("course not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone)]
pub struct CourseMeta {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub total_sentences: usize,
    pub total_drills: usize,
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
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

pub fn load_course(courses_dir: &std::path::Path, id: &str) -> Result<Course, StorageError> {
    let path = courses_dir.join(format!("{id}.json"));
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
    let bytes = serde_json::to_vec_pretty(course)?;
    let path = courses_dir.join(format!("{}.json", course.id));
    write_atomic(&path, &bytes)?;
    Ok(())
}

pub fn delete_course(courses_dir: &std::path::Path, id: &str) -> Result<(), StorageError> {
    let path = courses_dir.join(format!("{id}.json"));
    std::fs::remove_file(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StorageError::NotFound(id.into())
        } else {
            StorageError::Io(e)
        }
    })
}
