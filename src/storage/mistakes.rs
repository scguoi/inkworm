//! Global mistakes book: per-drill tracking of "answered wrong twice in a
//! row in normal flow → enters book → 3 qualifying study days clear it".
//!
//! The mistakes book is an independent practice channel: answers in
//! mistakes mode update streak_days but NOT mastered_count, and answers
//! in normal flow update wrong_streaks/promote-to-entries but NOT
//! streak_days.
//!
//! See spec: docs/superpowers/specs/2026-04-27-inkworm-mistakes-design.md

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::atomic::write_atomic;

pub const MISTAKES_SCHEMA_VERSION: u32 = 1;

/// Reference to one drill within one course.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DrillRef {
    #[serde(rename = "courseId")]
    pub course_id: String,
    #[serde(rename = "sentenceOrder")]
    pub sentence_order: u32,
    #[serde(rename = "drillStage")]
    pub drill_stage: u32,
}

/// Stable string key for BTreeMap lookups: `"course-id|sentence|stage"`.
/// Course ids are kebab-case (no `|`), so this is unambiguous.
pub type DrillKey = String;

/// Build the BTreeMap key for `wrong_streaks` lookups. Uses `|` as
/// separator since course ids are kebab-case (no `|`), so the key is
/// unambiguously parseable back into its three components if needed.
pub fn drill_key(d: &DrillRef) -> DrillKey {
    format!("{}|{}|{}", d.course_id, d.sentence_order, d.drill_stage)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MistakeBook {
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
    /// Lazy: only contains drills currently between "1 wrong" and either
    /// "next correct" (cleared) or "second wrong" (promoted to entries).
    #[serde(rename = "wrongStreaks", default)]
    pub wrong_streaks: BTreeMap<DrillKey, u32>,
    #[serde(default)]
    pub entries: Vec<MistakeEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MistakeEntry {
    pub drill: DrillRef,
    #[serde(rename = "enteredAt")]
    pub entered_at: DateTime<Utc>,
    /// 0..=2 persisted; reaching 3 triggers immediate removal from `entries`.
    #[serde(rename = "streakDays", default)]
    pub streak_days: u32,
    /// Most recent local date a qualifying-day +1 was applied to this entry.
    /// Prevents double-counting if both rounds correct then user re-attempts.
    #[serde(
        rename = "lastQualifiedDate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_qualified_date: Option<NaiveDate>,
    /// Today's two-round verdicts. Stale (different date) → replaced before
    /// any new write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub today: Option<TodayAttempts>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodayAttempts {
    pub date: NaiveDate,
    /// First-attempt verdict in round 1 today; None until attempted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round1: Option<bool>,
    /// First-attempt verdict in round 2 today; None until attempted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round2: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    /// The local date the session was launched on; determines whether a
    /// resumed session is still "today's" (mismatch on next launch → drop).
    #[serde(rename = "startedOn")]
    pub started_on: NaiveDate,
    /// Snapshot of entries at session start, plus any drills appended
    /// mid-session by `record_normal_attempt`.
    pub queue: Vec<DrillRef>,
    /// 1 or 2.
    #[serde(rename = "currentRound")]
    pub current_round: u8,
    /// Index into `queue` of the next drill to present in `current_round`.
    #[serde(rename = "nextIndex", default)]
    pub next_index: usize,
    /// Set true after round 1 completes; affects whether mid-session
    /// appended drills can still earn round1 results today.
    #[serde(rename = "round1Completed", default)]
    pub round1_completed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl MistakeBook {
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    schema_version: MISTAKES_SCHEMA_VERSION,
                    ..Self::default()
                });
            }
            Err(e) => return Err(e.into()),
        };
        let mut book: MistakeBook = serde_json::from_slice(&bytes)?;
        if book.schema_version == 0 {
            book.schema_version = MISTAKES_SCHEMA_VERSION;
        }
        Ok(book)
    }

    pub fn save(&self, path: &Path) -> Result<(), LoadError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn drill_a() -> DrillRef {
        DrillRef {
            course_id: "course-a".into(),
            sentence_order: 1,
            drill_stage: 2,
        }
    }

    #[test]
    fn drill_key_is_pipe_joined() {
        assert_eq!(drill_key(&drill_a()), "course-a|1|2");
    }

    #[test]
    fn empty_book_round_trips() {
        let mut b = MistakeBook::default();
        b.schema_version = MISTAKES_SCHEMA_VERSION;
        let json = serde_json::to_string(&b).unwrap();
        let b2: MistakeBook = serde_json::from_str(&json).unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn populated_book_round_trips_camel_case_keys() {
        let entry = MistakeEntry {
            drill: drill_a(),
            entered_at: Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap(),
            streak_days: 1,
            last_qualified_date: Some(chrono::NaiveDate::from_ymd_opt(2026, 4, 22).unwrap()),
            today: Some(TodayAttempts {
                date: chrono::NaiveDate::from_ymd_opt(2026, 4, 23).unwrap(),
                round1: Some(true),
                round2: None,
            }),
        };
        let mut b = MistakeBook {
            schema_version: MISTAKES_SCHEMA_VERSION,
            wrong_streaks: BTreeMap::new(),
            entries: vec![entry],
            session: Some(SessionState {
                started_on: chrono::NaiveDate::from_ymd_opt(2026, 4, 23).unwrap(),
                queue: vec![drill_a()],
                current_round: 1,
                next_index: 0,
                round1_completed: false,
            }),
        };
        b.wrong_streaks.insert("course-b|1|1".into(), 1);
        let json = serde_json::to_string(&b).unwrap();
        let b2: MistakeBook = serde_json::from_str(&json).unwrap();
        assert_eq!(b, b2);
        // Verify camelCase wire format.
        assert!(json.contains(r#""schemaVersion":1"#));
        assert!(json.contains(r#""wrongStreaks":"#));
        assert!(json.contains(r#""enteredAt":"#));
        assert!(json.contains(r#""streakDays":"#));
        assert!(json.contains(r#""lastQualifiedDate":"#));
        assert!(json.contains(r#""startedOn":"#));
        assert!(json.contains(r#""currentRound":"#));
        assert!(json.contains(r#""nextIndex":"#));
        assert!(json.contains(r#""round1Completed":"#));
    }

    #[test]
    fn load_missing_returns_empty_book() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mistakes.json");
        let b = MistakeBook::load(&path).unwrap();
        assert_eq!(b.schema_version, MISTAKES_SCHEMA_VERSION);
        assert!(b.entries.is_empty());
        assert!(b.wrong_streaks.is_empty());
        assert!(b.session.is_none());
    }

    #[test]
    fn save_then_load_preserves_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mistakes.json");
        let mut b = MistakeBook::default();
        b.schema_version = MISTAKES_SCHEMA_VERSION;
        b.wrong_streaks.insert("course-x|1|1".into(), 1);
        b.save(&path).unwrap();
        let b2 = MistakeBook::load(&path).unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn load_upgrades_zero_schema_version_to_current() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mistakes.json");
        std::fs::write(&path, b"{}").unwrap();
        let b = MistakeBook::load(&path).unwrap();
        assert_eq!(b.schema_version, MISTAKES_SCHEMA_VERSION);
    }
}
