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
pub enum MistakesError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl MistakeBook {
    pub fn empty() -> Self {
        Self {
            schema_version: MISTAKES_SCHEMA_VERSION,
            ..Self::default()
        }
    }

    pub fn load(path: &Path) -> Result<Self, MistakesError> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::empty());
            }
            Err(e) => return Err(e.into()),
        };
        let mut book: MistakeBook = serde_json::from_slice(&bytes)?;
        if book.schema_version == 0 {
            book.schema_version = MISTAKES_SCHEMA_VERSION;
        }
        Ok(book)
    }

    pub fn save(&self, path: &Path) -> Result<(), MistakesError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &bytes)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalOutcome {
    /// True iff this attempt promoted the drill from wrong_streaks to entries.
    pub promoted: bool,
}

impl MistakeBook {
    /// Record an answer in normal study mode. Updates wrong_streaks /
    /// entries / active session queue per spec §3.2. Does NOT touch
    /// `streak_days` / `today` / mastered_count.
    pub fn record_normal_attempt(
        &mut self,
        drill: &DrillRef,
        first_attempt_correct: bool,
        now_utc: DateTime<Utc>,
    ) -> NormalOutcome {
        let key = drill_key(drill);
        // Invariant 1: a drill in entries is never simultaneously in
        // wrong_streaks. Normal attempts on such a drill are invisible to
        // the mistakes book (decision 9).
        if self.entries.iter().any(|e| e.drill == *drill) {
            return NormalOutcome { promoted: false };
        }
        if first_attempt_correct {
            self.wrong_streaks.remove(&key);
            return NormalOutcome { promoted: false };
        }
        let count = self.wrong_streaks.entry(key.clone()).or_insert(0);
        *count += 1;
        if *count < 2 {
            return NormalOutcome { promoted: false };
        }
        self.wrong_streaks.remove(&key);
        self.entries.push(MistakeEntry {
            drill: drill.clone(),
            entered_at: now_utc,
            streak_days: 0,
            last_qualified_date: None,
            today: None,
        });
        sort_entries(&mut self.entries);
        // Caller precondition (enforced by startup + /mistakes command):
        // any non-None session has been verified as today's session before
        // reaching this function. We therefore append unconditionally; we
        // don't have a NaiveDate parameter and we don't need one.
        if let Some(session) = &mut self.session {
            session.queue.push(drill.clone());
        }
        NormalOutcome { promoted: true }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MistakesOutcome {
    /// True iff this attempt caused the drill to leave entries (streak 3).
    pub cleared: bool,
}

impl MistakeBook {
    /// Record an answer in mistakes mode for `round` (1 or 2). Implements
    /// spec §3.2 mistakes-mode branch. Returns `cleared = true` iff the
    /// drill reached streak 3 and was removed from entries.
    pub fn record_mistakes_attempt(
        &mut self,
        drill: &DrillRef,
        round: u8,
        first_attempt_correct: bool,
        today_local: NaiveDate,
    ) -> MistakesOutcome {
        let Some(idx) = self.entries.iter().position(|e| e.drill == *drill) else {
            return MistakesOutcome { cleared: false };
        };
        let entry = &mut self.entries[idx];

        // Refresh today if stale.
        let stale = entry.today.as_ref().map(|t| t.date) != Some(today_local);
        if stale {
            entry.today = Some(TodayAttempts {
                date: today_local,
                round1: None,
                round2: None,
            });
        }
        let today = entry.today.as_mut().expect("just set");

        // First-attempt-only: do not overwrite an existing slot.
        let slot = match round {
            1 => &mut today.round1,
            2 => &mut today.round2,
            _ => {
                debug_assert!(false, "round must be 1 or 2, got {round}");
                return MistakesOutcome { cleared: false };
            }
        };
        if slot.is_none() {
            *slot = Some(first_attempt_correct);
        }

        // Evaluate qualifying day: both rounds correct AND not already
        // counted today.
        let both_correct =
            matches!(today.round1, Some(true)) && matches!(today.round2, Some(true));
        if both_correct && entry.last_qualified_date != Some(today_local) {
            entry.streak_days += 1;
            entry.last_qualified_date = Some(today_local);
            if entry.streak_days >= 3 {
                self.entries.remove(idx);
                return MistakesOutcome { cleared: true };
            }
        }
        MistakesOutcome { cleared: false }
    }
}

fn sort_entries(entries: &mut [MistakeEntry]) {
    entries.sort_by(|a, b| {
        a.entered_at
            .cmp(&b.entered_at)
            .then_with(|| a.drill.course_id.cmp(&b.drill.course_id))
            .then_with(|| a.drill.sentence_order.cmp(&b.drill.sentence_order))
            .then_with(|| a.drill.drill_stage.cmp(&b.drill.drill_stage))
    });
}

impl MistakeBook {
    /// Idempotent: ensures `session` is a valid in-progress session for
    /// `today` if entries are non-empty. Returns true iff a NEW session
    /// was started this call (vs. resumed/no-op).
    pub fn ensure_session(&mut self, today_local: NaiveDate) -> bool {
        // Drop stale session from a previous day.
        if let Some(s) = &self.session {
            if s.started_on != today_local {
                self.session = None;
            }
        }
        if self.entries.is_empty() {
            return false;
        }
        if self.session.is_some() {
            return false;
        }
        self.session = Some(SessionState {
            started_on: today_local,
            queue: self.entries.iter().map(|e| e.drill.clone()).collect(),
            current_round: 1,
            next_index: 0,
            round1_completed: false,
        });
        true
    }

    /// Returns the drill that should be presented now, advancing past any
    /// cleared/orphaned queue slots silently. Returns None when the
    /// session has finished (and clears `self.session`).
    ///
    /// Takes `&mut self` because it normalizes session state (skips orphans,
    /// transitions round 1→2, clears completed sessions). Render paths that
    /// only need a snapshot should use [`Self::session_progress`] for
    /// progress numbers; a `&self` accessor for the current drill ref is
    /// planned in a later task once render-time normalization isn't needed.
    pub fn peek_current_drill(&mut self) -> Option<DrillRef> {
        loop {
            let session = self.session.as_ref()?;
            if session.next_index >= session.queue.len() {
                // End of current round.
                if session.current_round == 1 {
                    let s = self.session.as_mut().unwrap();
                    s.round1_completed = true;
                    s.current_round = 2;
                    s.next_index = 0;
                    continue;
                } else {
                    debug_assert_eq!(session.current_round, 2, "current_round must be 1 or 2");
                    self.session = None;
                    return None;
                }
            }
            let drill = session.queue[session.next_index].clone();
            if self.entries.iter().any(|e| e.drill == drill) {
                return Some(drill);
            }
            // Skip cleared/orphaned drill.
            self.session.as_mut().unwrap().next_index += 1;
        }
    }

    /// Move past the current drill (caller has finished evaluating it).
    pub fn advance_session(&mut self) {
        if let Some(s) = self.session.as_mut() {
            s.next_index += 1;
        }
        // Pre-normalize persisted state so a `save()` between advance and the
        // next peek writes a clean next_index (skipping any drills that were
        // cleared during this advance).
        let _ = self.peek_current_drill();
    }

    /// Current round/index/length for top-bar rendering. None if no
    /// session or session just completed.
    pub fn session_progress(&self) -> Option<SessionProgress> {
        let s = self.session.as_ref()?;
        Some(SessionProgress {
            round: s.current_round,
            index: s.next_index,
            total: s.queue.len(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionProgress {
    pub round: u8,
    pub index: usize,
    pub total: usize,
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

    fn drill_b() -> DrillRef {
        DrillRef {
            course_id: "course-b".into(),
            sentence_order: 2,
            drill_stage: 1,
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap()
    }

    #[test]
    fn normal_correct_clears_wrong_streak() {
        let mut b = MistakeBook::default();
        b.wrong_streaks.insert(drill_key(&drill_a()), 1);
        let outcome = b.record_normal_attempt(&drill_a(), true, now());
        assert!(!outcome.promoted);
        assert!(b.wrong_streaks.is_empty());
        assert!(b.entries.is_empty());
    }

    #[test]
    fn normal_first_wrong_inserts_count_one() {
        let mut b = MistakeBook::default();
        let outcome = b.record_normal_attempt(&drill_a(), false, now());
        assert!(!outcome.promoted);
        assert_eq!(b.wrong_streaks.get(&drill_key(&drill_a())), Some(&1));
        assert!(b.entries.is_empty());
    }

    #[test]
    fn normal_second_wrong_promotes_to_entries() {
        let mut b = MistakeBook::default();
        b.record_normal_attempt(&drill_a(), false, now());
        let outcome = b.record_normal_attempt(&drill_a(), false, now());
        assert!(outcome.promoted);
        assert!(b.wrong_streaks.is_empty());
        assert_eq!(b.entries.len(), 1);
        assert_eq!(b.entries[0].drill, drill_a());
        assert_eq!(b.entries[0].streak_days, 0);
    }

    #[test]
    fn normal_attempt_on_drill_already_in_entries_is_noop_for_book_state() {
        let mut b = MistakeBook::default();
        b.entries.push(MistakeEntry {
            drill: drill_a(),
            entered_at: now(),
            streak_days: 1,
            last_qualified_date: None,
            today: None,
        });
        // Wrong attempt in normal flow on a drill already in entries: must NOT
        // touch wrong_streaks or entries (invariant: disjoint sets).
        let outcome = b.record_normal_attempt(&drill_a(), false, now());
        assert!(!outcome.promoted);
        assert!(b.wrong_streaks.is_empty());
        assert_eq!(b.entries.len(), 1);
        assert_eq!(b.entries[0].streak_days, 1);
    }

    #[test]
    fn promoted_drill_appends_to_active_session_queue() {
        let mut b = MistakeBook::default();
        b.session = Some(SessionState {
            started_on: chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            queue: vec![drill_b()],
            current_round: 1,
            next_index: 0,
            round1_completed: false,
        });
        b.record_normal_attempt(&drill_a(), false, now());
        let o = b.record_normal_attempt(&drill_a(), false, now());
        assert!(o.promoted);
        let session = b.session.as_ref().unwrap();
        assert_eq!(session.queue, vec![drill_b(), drill_a()]);
    }

    #[test]
    fn entries_sorted_by_entered_at_then_drill_ref() {
        let mut b = MistakeBook::default();
        let later = Utc.with_ymd_and_hms(2026, 4, 28, 0, 0, 0).unwrap();
        // Promote drill_b first (earlier timestamp).
        b.record_normal_attempt(&drill_b(), false, now());
        b.record_normal_attempt(&drill_b(), false, now());
        // Promote drill_a later.
        b.record_normal_attempt(&drill_a(), false, later);
        b.record_normal_attempt(&drill_a(), false, later);
        assert_eq!(
            b.entries.iter().map(|e| e.drill.clone()).collect::<Vec<_>>(),
            vec![drill_b(), drill_a()]
        );
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
        let mut b = MistakeBook::empty();
        b.wrong_streaks.insert("course-x|1|1".into(), 1);
        b.save(&path).unwrap();
        let b2 = MistakeBook::load(&path).unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn normal_correct_on_never_seen_drill_is_full_noop() {
        let mut b = MistakeBook::default();
        let outcome = b.record_normal_attempt(&drill_a(), true, now());
        assert!(!outcome.promoted);
        assert!(b.wrong_streaks.is_empty());
        assert!(b.entries.is_empty());
        assert!(b.session.is_none());
    }

    #[test]
    fn load_upgrades_zero_schema_version_to_current() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mistakes.json");
        std::fs::write(&path, b"{}").unwrap();
        let b = MistakeBook::load(&path).unwrap();
        assert_eq!(b.schema_version, MISTAKES_SCHEMA_VERSION);
    }

    fn d(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn entry_for(drill: DrillRef, t: DateTime<Utc>) -> MistakeEntry {
        MistakeEntry {
            drill,
            entered_at: t,
            streak_days: 0,
            last_qualified_date: None,
            today: None,
        }
    }

    fn book_with_one_entry(streak: u32, last_q: Option<chrono::NaiveDate>) -> MistakeBook {
        MistakeBook {
            schema_version: MISTAKES_SCHEMA_VERSION,
            wrong_streaks: BTreeMap::new(),
            entries: vec![MistakeEntry {
                drill: drill_a(),
                entered_at: now(),
                streak_days: streak,
                last_qualified_date: last_q,
                today: None,
            }],
            session: None,
        }
    }

    #[test]
    fn mistakes_round1_correct_then_round2_correct_qualifies_day() {
        let mut b = book_with_one_entry(0, None);
        let o1 = b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
        assert!(!o1.cleared);
        let o2 = b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
        assert!(!o2.cleared);
        let entry = &b.entries[0];
        assert_eq!(entry.streak_days, 1);
        assert_eq!(entry.last_qualified_date, Some(d("2026-04-27")));
        let today = entry.today.as_ref().unwrap();
        assert_eq!(today.round1, Some(true));
        assert_eq!(today.round2, Some(true));
    }

    #[test]
    fn mistakes_first_attempt_wins_retry_does_not_overwrite() {
        let mut b = book_with_one_entry(0, None);
        b.record_mistakes_attempt(&drill_a(), 1, false, d("2026-04-27"));
        b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
        let today = b.entries[0].today.as_ref().unwrap();
        assert_eq!(today.round1, Some(false));
    }

    #[test]
    fn mistakes_wrong_round_does_not_decrement_streak() {
        let mut b = book_with_one_entry(2, Some(d("2026-04-26")));
        b.record_mistakes_attempt(&drill_a(), 1, false, d("2026-04-27"));
        b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
        assert_eq!(b.entries[0].streak_days, 2);
    }

    #[test]
    fn mistakes_qualifying_day_does_not_double_count_in_same_day() {
        let mut b = book_with_one_entry(0, None);
        b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
        b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
        // Hypothetical re-attempt of round 2 (e.g., from a re-launched session
        // edge case). last_qualified_date guards.
        b.entries[0].today.as_mut().unwrap().round2 = Some(true);
        // No further +1 should occur because last_qualified_date == today.
        assert_eq!(b.entries[0].streak_days, 1);
    }

    #[test]
    fn mistakes_third_qualifying_day_clears_drill_from_entries() {
        let mut b = book_with_one_entry(2, Some(d("2026-04-26")));
        b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
        let o = b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
        assert!(o.cleared);
        assert!(b.entries.is_empty());
    }

    #[test]
    fn mistakes_both_rounds_wrong_does_not_qualify() {
        let mut b = book_with_one_entry(1, Some(d("2026-04-26")));
        b.record_mistakes_attempt(&drill_a(), 1, false, d("2026-04-27"));
        b.record_mistakes_attempt(&drill_a(), 2, false, d("2026-04-27"));
        let entry = &b.entries[0];
        assert_eq!(entry.streak_days, 1);
        assert_eq!(entry.today.as_ref().unwrap().round1, Some(false));
        assert_eq!(entry.today.as_ref().unwrap().round2, Some(false));
    }

    #[test]
    fn mistakes_today_resets_when_date_changes() {
        let mut b = book_with_one_entry(0, None);
        b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
        b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
        // Next day:
        b.record_mistakes_attempt(&drill_a(), 1, false, d("2026-04-28"));
        let today = b.entries[0].today.as_ref().unwrap();
        assert_eq!(today.date, d("2026-04-28"));
        assert_eq!(today.round1, Some(false));
        assert_eq!(today.round2, None);
    }

    #[test]
    fn mistakes_attempt_on_unknown_drill_is_noop() {
        let mut b = MistakeBook::default();
        let o = b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
        assert!(!o.cleared);
        assert!(b.entries.is_empty());
    }

    #[test]
    fn ensure_session_starts_when_entries_nonempty_and_no_session() {
        let mut b = MistakeBook::default();
        b.entries.push(entry_for(drill_a(), now()));
        let started = b.ensure_session(d("2026-04-27"));
        assert!(started);
        let s = b.session.as_ref().unwrap();
        assert_eq!(s.started_on, d("2026-04-27"));
        assert_eq!(s.current_round, 1);
        assert_eq!(s.next_index, 0);
        assert!(!s.round1_completed);
        assert_eq!(s.queue, vec![drill_a()]);
    }

    #[test]
    fn ensure_session_no_op_when_entries_empty() {
        let mut b = MistakeBook::default();
        let started = b.ensure_session(d("2026-04-27"));
        assert!(!started);
        assert!(b.session.is_none());
    }

    #[test]
    fn ensure_session_drops_stale_session_from_yesterday() {
        let mut b = MistakeBook::default();
        b.entries.push(entry_for(drill_a(), now()));
        b.session = Some(SessionState {
            started_on: d("2026-04-26"),
            queue: vec![drill_a()],
            current_round: 2,
            next_index: 1,
            round1_completed: true,
        });
        b.ensure_session(d("2026-04-27"));
        let s = b.session.as_ref().unwrap();
        assert_eq!(s.started_on, d("2026-04-27"));
        assert_eq!(s.current_round, 1);
        assert_eq!(s.next_index, 0);
    }

    #[test]
    fn ensure_session_resumes_today_session_in_place() {
        let mut b = MistakeBook::default();
        b.entries.push(entry_for(drill_a(), now()));
        let same = SessionState {
            started_on: d("2026-04-27"),
            queue: vec![drill_a()],
            current_round: 2,
            next_index: 0,
            round1_completed: true,
        };
        b.session = Some(same.clone());
        b.ensure_session(d("2026-04-27"));
        assert_eq!(b.session.as_ref().unwrap(), &same);
    }

    #[test]
    fn advance_session_walks_round_1_then_round_2_then_completes() {
        let mut b = MistakeBook::default();
        b.entries.push(entry_for(drill_a(), now()));
        b.entries.push(entry_for(drill_b(), now()));
        b.ensure_session(d("2026-04-27"));
        // Round 1: drill_a then drill_b.
        assert_eq!(b.peek_current_drill(), Some(drill_a()));
        b.advance_session();
        assert_eq!(b.peek_current_drill(), Some(drill_b()));
        b.advance_session();
        // Round 2 starts.
        let s = b.session.as_ref().unwrap();
        assert_eq!(s.current_round, 2);
        assert_eq!(s.next_index, 0);
        assert!(s.round1_completed);
        assert_eq!(b.peek_current_drill(), Some(drill_a()));
        b.advance_session();
        assert_eq!(b.peek_current_drill(), Some(drill_b()));
        b.advance_session();
        // Session done → cleared.
        assert!(b.session.is_none());
        assert!(b.peek_current_drill().is_none());
    }

    #[test]
    fn advance_session_skips_drills_no_longer_in_entries() {
        // Drill cleared mid-session: queue still has it but entries lost it.
        let mut b = MistakeBook::default();
        b.entries.push(entry_for(drill_a(), now()));
        b.entries.push(entry_for(drill_b(), now()));
        b.ensure_session(d("2026-04-27"));
        // Pretend drill_a got cleared.
        b.entries.retain(|e| e.drill != drill_a());
        // First peek should skip drill_a and return drill_b.
        assert_eq!(b.peek_current_drill(), Some(drill_b()));
    }

    #[test]
    fn peek_returns_none_when_all_drills_cleared_mid_session() {
        let mut b = MistakeBook::default();
        b.entries.push(entry_for(drill_a(), now()));
        b.entries.push(entry_for(drill_b(), now()));
        b.ensure_session(d("2026-04-27"));
        b.entries.clear(); // all cleared simultaneously
        assert_eq!(b.peek_current_drill(), None);
        assert!(b.session.is_none());
    }
}
