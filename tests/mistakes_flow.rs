//! End-to-end mistakes book flow over multiple days.
//!
//! Pure state-level test (no TUI): drives MistakeBook via the same
//! public API the App uses, asserts entry/streak/clear lifecycle.

use chrono::{NaiveDate, TimeZone, Utc};
use inkworm::storage::mistakes::{drill_key, DrillRef, MistakeBook};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn dr(course: &str, sentence: u32, stage: u32) -> DrillRef {
    DrillRef {
        course_id: course.into(),
        sentence_order: sentence,
        drill_stage: stage,
    }
}

#[test]
fn full_lifecycle_enter_three_days_clear_then_re_enter() {
    let mut b = MistakeBook::default();
    let drill = dr("course-a", 1, 2);
    let now = Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap();

    // Day 1: enter via 2 consecutive wrong in normal flow.
    b.record_normal_attempt(&drill, false, now);
    let o = b.record_normal_attempt(&drill, false, now);
    assert!(o.promoted);
    assert_eq!(b.entries.len(), 1);

    // Day 2: launch session, both rounds correct → streak +1.
    b.ensure_session(d("2026-04-28"));
    assert_eq!(b.peek_current_drill(), Some(drill.clone()));
    b.record_mistakes_attempt(&drill, 1, true, d("2026-04-28"));
    b.advance_session();
    assert_eq!(b.peek_current_drill(), Some(drill.clone())); // round 2 starts
    let o = b.record_mistakes_attempt(&drill, 2, true, d("2026-04-28"));
    assert!(!o.cleared);
    b.advance_session();
    assert!(b.session.is_none());
    assert_eq!(b.entries[0].streak_days, 1);

    // Day 3 (skipped) — Day 4: launch new session, both correct → streak 2.
    b.ensure_session(d("2026-04-30"));
    b.record_mistakes_attempt(&drill, 1, true, d("2026-04-30"));
    b.advance_session();
    b.record_mistakes_attempt(&drill, 2, true, d("2026-04-30"));
    b.advance_session();
    assert_eq!(b.entries[0].streak_days, 2);

    // Day 5: launch session, both correct → streak 3 → cleared.
    b.ensure_session(d("2026-05-01"));
    b.record_mistakes_attempt(&drill, 1, true, d("2026-05-01"));
    b.advance_session();
    let o = b.record_mistakes_attempt(&drill, 2, true, d("2026-05-01"));
    assert!(o.cleared);
    b.advance_session();
    assert!(b.entries.is_empty());

    // Day 6: re-error twice → re-enter (no immunity).
    let later = Utc.with_ymd_and_hms(2026, 5, 2, 10, 0, 0).unwrap();
    b.record_normal_attempt(&drill, false, later);
    b.record_normal_attempt(&drill, false, later);
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].streak_days, 0);
}

#[test]
fn cross_course_mix_and_purge() {
    let mut b = MistakeBook::default();
    let a = dr("course-a", 1, 1);
    let b1 = dr("course-b", 1, 1);
    let now = Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap();
    b.record_normal_attempt(&a, false, now);
    b.record_normal_attempt(&a, false, now);
    let later = Utc.with_ymd_and_hms(2026, 4, 27, 11, 0, 0).unwrap();
    b.record_normal_attempt(&b1, false, later);
    b.record_normal_attempt(&b1, false, later);
    assert_eq!(b.entries.len(), 2);
    assert_eq!(b.entries[0].drill, a); // earlier entered_at first
    b.purge_course("course-a");
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].drill, b1);
    assert!(!b.wrong_streaks.contains_key(&drill_key(&a)));
}

#[test]
fn wrong_round_does_not_clear_existing_streak() {
    let mut b = MistakeBook::default();
    let drill = dr("course-a", 1, 1);
    let now = Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap();
    b.record_normal_attempt(&drill, false, now);
    b.record_normal_attempt(&drill, false, now);
    // Get to streak 2 over two days first.
    for day in ["2026-04-28", "2026-04-29"] {
        b.ensure_session(d(day));
        b.record_mistakes_attempt(&drill, 1, true, d(day));
        b.advance_session();
        b.record_mistakes_attempt(&drill, 2, true, d(day));
        b.advance_session();
    }
    assert_eq!(b.entries[0].streak_days, 2);
    // Day 3: round 1 wrong → no +1, but streak NOT reset.
    b.ensure_session(d("2026-04-30"));
    b.record_mistakes_attempt(&drill, 1, false, d("2026-04-30"));
    b.advance_session();
    b.record_mistakes_attempt(&drill, 2, true, d("2026-04-30"));
    b.advance_session();
    assert_eq!(b.entries[0].streak_days, 2);
}

#[test]
fn mid_session_appended_drill_in_round1_gets_two_attempts() {
    let mut b = MistakeBook::default();
    let a = dr("course-a", 1, 1);
    let b1 = dr("course-b", 1, 1);
    let now = Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap();
    b.record_normal_attempt(&a, false, now);
    b.record_normal_attempt(&a, false, now);
    b.ensure_session(d("2026-04-28"));
    // Round 1 in progress at index 0, drill_a.
    // Mid-round-1, drill_b promotes via normal flow.
    let later = Utc.with_ymd_and_hms(2026, 4, 28, 10, 0, 0).unwrap();
    b.record_normal_attempt(&b1, false, later);
    b.record_normal_attempt(&b1, false, later);
    // queue should now contain [drill_a, drill_b] for round 1, then again
    // both for round 2.
    assert_eq!(b.peek_current_drill(), Some(a.clone()));
    b.advance_session();
    assert_eq!(b.peek_current_drill(), Some(b1.clone()));
    b.advance_session();
    // Round 2 starts from index 0 → drill_a, drill_b again.
    assert_eq!(b.peek_current_drill(), Some(a.clone()));
    b.advance_session();
    assert_eq!(b.peek_current_drill(), Some(b1.clone()));
    b.advance_session();
    assert!(b.session.is_none());
}
