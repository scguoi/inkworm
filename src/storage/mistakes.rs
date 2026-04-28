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
        let both_correct = matches!(today.round1, Some(true)) && matches!(today.round2, Some(true));
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

    /// Non-mutating peek for read-only contexts (e.g. UI rendering).
    /// Returns the current drill_ref by raw lookup (does NOT skip
    /// cleared/orphaned slots — for that use the &mut peek_current_drill).
    pub fn current_drill_ref(&self) -> Option<DrillRef> {
        let s = self.session.as_ref()?;
        s.queue.get(s.next_index).cloned()
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

impl MistakeBook {
    /// Drop entries pointing at courses/sentences/stages that no longer
    /// exist. `provider` returns the Course for an id, or None if missing.
    pub fn prune_orphans<'a, F>(&mut self, mut provider: F)
    where
        F: FnMut(&str) -> Option<&'a crate::storage::course::Course>,
    {
        // Drop wrong_streaks for courses that no longer exist. Granularity matches
        // purge_course (course-level): a wrong_streaks key whose course_id has no
        // provider entry is a ghost, drop it.
        self.wrong_streaks.retain(|k, _| {
            let course_id = k.split('|').next().unwrap_or("");
            provider(course_id).is_some()
        });
        self.entries.retain(|e| {
            let Some(course) = provider(&e.drill.course_id) else {
                return false;
            };
            let Some(sentence) = course
                .sentences
                .iter()
                .find(|s| s.order == e.drill.sentence_order)
            else {
                return false;
            };
            sentence
                .drills
                .iter()
                .any(|d| d.stage == e.drill.drill_stage)
        });
        // Also prune session queue: any drill whose entry is gone is now an
        // orphan. Use the same clean 2-pass retain pattern as `purge_course`.
        // Build live set before the if-let to avoid simultaneous borrow of
        // self.entries (shared) and self.session (mutable).
        let live: std::collections::HashSet<DrillKey> =
            self.entries.iter().map(|e| drill_key(&e.drill)).collect();
        if let Some(session) = self.session.as_mut() {
            let shift_next = session.queue[..session.next_index]
                .iter()
                .filter(|d| !live.contains(&drill_key(d)))
                .count();
            // (saturating_sub not needed: shift_next <= session.next_index by construction)
            session.next_index -= shift_next;
            session.queue.retain(|d| live.contains(&drill_key(d)));
            if session.queue.is_empty() {
                self.session = None;
            }
        }
    }

    /// Remove all traces of `course_id` from the book. Adjusts session
    /// queue and `next_index`; clears session if the queue is exhausted.
    pub fn purge_course(&mut self, course_id: &str) {
        let prefix = format!("{course_id}|");
        self.wrong_streaks.retain(|k, _| !k.starts_with(&prefix));
        self.entries.retain(|e| e.drill.course_id != course_id);
        let Some(session) = self.session.as_mut() else {
            return;
        };
        // Pass 1: count purged items strictly before next_index.
        // (saturating_sub not needed: shift_next <= session.next_index by construction)
        let shift_next = session.queue[..session.next_index]
            .iter()
            .filter(|d| d.course_id == course_id)
            .count();
        session.next_index -= shift_next;
        // Pass 2: drop purged items in place.
        session.queue.retain(|d| d.course_id != course_id);
        if session.queue.is_empty() {
            self.session = None;
        }
    }
}

#[cfg(test)]
mod tests;
