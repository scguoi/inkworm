//! Per-user study progress, keyed by course id.
//!
//! Written once on exit from the Study screen (or on course completion).
//! Derived fields (`total_drills`, `completed_drills`) are computed on demand
//! from the current Course + Progress, never persisted.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::storage::atomic::write_atomic;
use crate::storage::course::{Course, StorageError};

pub const PROGRESS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Progress {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(
        rename = "activeCourseId",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub active_course_id: Option<String>,
    #[serde(default)]
    pub courses: BTreeMap<String, CourseProgress>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CourseProgress {
    // chrono ≥0.4.19 implements Default for DateTime<Utc> (returns the Unix epoch),
    // which is what we want for "never studied".
    #[serde(rename = "lastStudiedAt")]
    pub last_studied_at: DateTime<Utc>,
    /// Keyed by sentence `order` as a decimal string.
    #[serde(default)]
    pub sentences: BTreeMap<String, SentenceProgress>,
    /// How many times the user has reached the end of this course. Bumped
    /// once per course-mode pass through `next_drill`'s final step.
    /// Survives the relearn reset done in `reset_course_progress`.
    #[serde(rename = "completionCount", default)]
    pub completion_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SentenceProgress {
    /// Keyed by drill `stage` as a decimal string.
    #[serde(default)]
    pub drills: BTreeMap<String, DrillProgress>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DrillProgress {
    #[serde(rename = "masteredCount")]
    pub mastered_count: u32,
    #[serde(
        rename = "lastCorrectAt",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub last_correct_at: Option<DateTime<Utc>>,
}

impl Progress {
    pub fn empty() -> Self {
        Self {
            schema_version: PROGRESS_SCHEMA_VERSION,
            active_course_id: None,
            courses: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, StorageError> {
        // Translate NotFound at the IO call site (no exists()+read TOCTOU window);
        // consistent with load_course / delete_course.
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => return Err(e.into()),
        };
        let mut p: Progress = serde_json::from_slice(&bytes)?;
        if p.schema_version == 0 {
            p.schema_version = PROGRESS_SCHEMA_VERSION;
        }
        Ok(p)
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &bytes)?;
        Ok(())
    }

    pub fn course(&self, id: &str) -> Option<&CourseProgress> {
        self.courses.get(id)
    }

    pub fn course_mut(&mut self, id: &str) -> &mut CourseProgress {
        self.courses.entry(id.to_string()).or_default()
    }

    /// Clear every drill's `mastered_count` (and `last_correct_at`) for the
    /// given course so a re-entry shows a fresh pass. No-op if the course
    /// has no progress recorded.
    pub fn reset_course_progress(&mut self, course_id: &str) {
        let Some(cp) = self.courses.get_mut(course_id) else {
            return;
        };
        for sp in cp.sentences.values_mut() {
            for dp in sp.drills.values_mut() {
                dp.mastered_count = 0;
                dp.last_correct_at = None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CourseStats {
    pub total_drills: usize,
    pub completed_drills: usize,
}

impl CourseStats {
    pub fn percent(&self) -> u32 {
        if self.total_drills == 0 {
            0
        } else {
            ((self.completed_drills as f64 / self.total_drills as f64) * 100.0).round() as u32
        }
    }
}

pub fn course_stats(course: &Course, progress: Option<&CourseProgress>) -> CourseStats {
    let total = course.sentences.iter().map(|s| s.drills.len()).sum();
    let completed = match progress {
        None => 0,
        Some(cp) => cp
            .sentences
            .values()
            .flat_map(|sp| sp.drills.values())
            .filter(|dp| dp.mastered_count >= 1)
            .count(),
    };
    CourseStats {
        total_drills: total,
        completed_drills: completed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn empty_round_trips() {
        let p = Progress::empty();
        let json = serde_json::to_string(&p).unwrap();
        let p2: Progress = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn serde_uses_camel_case_keys() {
        let mut p = Progress::empty();
        p.active_course_id = Some("x".into());
        let cp = p.course_mut("x");
        cp.last_studied_at = Utc.with_ymd_and_hms(2026, 4, 21, 0, 0, 0).unwrap();
        cp.sentences.insert("1".into(), SentenceProgress::default());
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""schemaVersion":1"#));
        assert!(json.contains(r#""activeCourseId":"x""#));
        assert!(json.contains(r#""lastStudiedAt":"2026-04-21T00:00:00Z""#));
    }

    #[test]
    fn reset_course_progress_clears_every_drill() {
        let mut p = Progress::empty();
        let cp = p.course_mut("c1");
        let now = Utc.with_ymd_and_hms(2026, 4, 21, 0, 0, 0).unwrap();
        cp.last_studied_at = now;
        cp.completion_count = 2;
        let mut sp = SentenceProgress::default();
        sp.drills.insert(
            "1".into(),
            DrillProgress {
                mastered_count: 3,
                last_correct_at: Some(now),
            },
        );
        sp.drills.insert(
            "2".into(),
            DrillProgress {
                mastered_count: 1,
                last_correct_at: Some(now),
            },
        );
        cp.sentences.insert("1".into(), sp);

        // Unrelated course must stay untouched.
        let cp2 = p.course_mut("c2");
        cp2.sentences
            .insert("1".into(), SentenceProgress::default());
        cp2.sentences.get_mut("1").unwrap().drills.insert(
            "1".into(),
            DrillProgress {
                mastered_count: 5,
                last_correct_at: Some(now),
            },
        );

        p.reset_course_progress("c1");

        let drills = &p.courses["c1"].sentences["1"].drills;
        assert_eq!(drills["1"].mastered_count, 0);
        assert_eq!(drills["1"].last_correct_at, None);
        assert_eq!(drills["2"].mastered_count, 0);
        assert_eq!(drills["2"].last_correct_at, None);
        // last_studied_at is preserved — relearn shouldn't rewrite history.
        assert_eq!(p.courses["c1"].last_studied_at, now);
        // completion_count is preserved — the cumulative-pass counter survives
        // the relearn reset (it's the very signal "over-learned" depends on).
        assert_eq!(p.courses["c1"].completion_count, 2);
        // Other courses untouched.
        assert_eq!(p.courses["c2"].sentences["1"].drills["1"].mastered_count, 5);
    }

    #[test]
    fn reset_course_progress_is_noop_for_unknown_course() {
        let mut p = Progress::empty();
        p.reset_course_progress("ghost");
        assert!(p.courses.is_empty());
    }

}
