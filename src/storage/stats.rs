//! Per-day learning statistics: active study time, submits, accuracy, words.
//!
//! See spec: docs/superpowers/specs/2026-05-13-learning-stats-design.md

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::storage::atomic::write_atomic;
use crate::storage::course::StorageError;

pub const STATS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Stats {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(default)]
    pub days: BTreeMap<NaiveDate, DayStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DayStats {
    #[serde(rename = "activeMs", default)]
    pub active_ms: u64,
    #[serde(default)]
    pub submits: u32,
    #[serde(default)]
    pub correct: u32,
    #[serde(default)]
    pub words: u32,
}

impl Stats {
    pub fn empty() -> Self {
        Self {
            schema_version: STATS_SCHEMA_VERSION,
            days: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, StorageError> {
        // Mirror Progress::load: NotFound → empty, no exists()+read TOCTOU.
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => return Err(e.into()),
        };
        let mut s: Stats = serde_json::from_slice(&bytes)?;
        if s.schema_version == 0 {
            s.schema_version = STATS_SCHEMA_VERSION;
        }
        Ok(s)
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn empty_round_trips() {
        let s = Stats::empty();
        let json = serde_json::to_string(&s).unwrap();
        let s2: Stats = serde_json::from_str(&json).unwrap();
        assert_eq!(s, s2);
        assert_eq!(s2.schema_version, 1);
    }

    #[test]
    fn serde_uses_camel_case_keys() {
        let mut s = Stats::empty();
        let date = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
        s.days.insert(
            date,
            DayStats {
                active_ms: 12_345,
                submits: 24,
                correct: 22,
                words: 168,
            },
        );
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""schemaVersion":1"#));
        assert!(json.contains(r#""2026-05-13""#));
        assert!(json.contains(r#""activeMs":12345"#));
        assert!(json.contains(r#""submits":24"#));
    }

    #[test]
    fn load_missing_returns_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let s = Stats::load(&path).unwrap();
        assert_eq!(s, Stats::empty());
    }

    #[test]
    fn day_stats_default_is_zeroed() {
        let d = DayStats::default();
        assert_eq!(d.active_ms, 0);
        assert_eq!(d.submits, 0);
        assert_eq!(d.correct, 0);
        assert_eq!(d.words, 0);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("stats.json");
        let mut s = Stats::empty();
        let date = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
        s.days.insert(
            date,
            DayStats {
                active_ms: 743_000,
                submits: 24,
                correct: 22,
                words: 168,
            },
        );
        s.save(&path).unwrap();
        let s2 = Stats::load(&path).unwrap();
        assert_eq!(s, s2);
    }
}
