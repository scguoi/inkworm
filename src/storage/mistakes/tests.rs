use super::*;
use chrono::{DateTime, TimeZone, Utc};
use std::collections::BTreeMap;

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
    let mut b = MistakeBook {
        session: Some(SessionState {
            started_on: chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            queue: vec![drill_b()],
            current_round: 1,
            next_index: 0,
            round1_completed: false,
        }),
        ..Default::default()
    };
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
        b.entries
            .iter()
            .map(|e| e.drill.clone())
            .collect::<Vec<_>>(),
        vec![drill_b(), drill_a()]
    );
}

#[test]
fn drill_key_is_pipe_joined() {
    assert_eq!(drill_key(&drill_a()), "course-a|1|2");
}

#[test]
fn empty_book_round_trips() {
    let b = MistakeBook {
        schema_version: MISTAKES_SCHEMA_VERSION,
        ..Default::default()
    };
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
fn ensure_session_skips_when_all_entries_two_rounds_done_today() {
    // Regression: after the user finishes round 1+2 today,
    // peek_current_drill clears `session`. A relaunch on the same day
    // must not auto-recreate a fresh round-1 session — repeats wouldn't
    // change state (first-attempt-only) and would just pull the user
    // back into mistakes mode with no purpose.
    let mut b = MistakeBook::default();
    let mut entry = entry_for(drill_a(), now());
    entry.streak_days = 1;
    entry.last_qualified_date = Some(d("2026-04-27"));
    entry.today = Some(TodayAttempts {
        date: d("2026-04-27"),
        round1: Some(true),
        round2: Some(true),
    });
    b.entries.push(entry);
    let started = b.ensure_session(d("2026-04-27"));
    assert!(!started);
    assert!(b.session.is_none());
}

#[test]
fn ensure_session_skips_when_today_done_even_for_failed_attempts() {
    // Two rounds attempted today (regardless of correctness) is enough
    // to skip auto-pop: subsequent attempts can't change today's
    // verdict either way.
    let mut b = MistakeBook::default();
    let mut entry = entry_for(drill_a(), now());
    entry.today = Some(TodayAttempts {
        date: d("2026-04-27"),
        round1: Some(false),
        round2: Some(true),
    });
    b.entries.push(entry);
    let started = b.ensure_session(d("2026-04-27"));
    assert!(!started);
    assert!(b.session.is_none());
}

#[test]
fn ensure_session_force_creates_session_even_when_today_done() {
    // /mistakes palette path: user wants to re-practice even though
    // today's already counted.
    let mut b = MistakeBook::default();
    let mut entry = entry_for(drill_a(), now());
    entry.today = Some(TodayAttempts {
        date: d("2026-04-27"),
        round1: Some(true),
        round2: Some(true),
    });
    b.entries.push(entry);
    let started = b.ensure_session_force(d("2026-04-27"));
    assert!(started);
    let s = b.session.as_ref().unwrap();
    assert_eq!(s.current_round, 1);
    assert_eq!(s.next_index, 0);
}

#[test]
fn ensure_session_drops_phantom_session_when_today_already_done() {
    // Regression for the on-disk state shape produced by the old
    // ensure_session: a fresh round-1 session got persisted *after*
    // today's two rounds were already complete. New launches must
    // reclaim that phantom and return the user to Course mode.
    let mut b = MistakeBook::default();
    let mut entry = entry_for(drill_a(), now());
    entry.streak_days = 1;
    entry.last_qualified_date = Some(d("2026-04-27"));
    entry.today = Some(TodayAttempts {
        date: d("2026-04-27"),
        round1: Some(true),
        round2: Some(true),
    });
    b.entries.push(entry);
    b.session = Some(SessionState {
        started_on: d("2026-04-27"),
        queue: vec![drill_a()],
        current_round: 1,
        next_index: 0,
        round1_completed: false,
    });
    let started = b.ensure_session(d("2026-04-27"));
    assert!(!started);
    assert!(b.session.is_none(), "phantom session should be dropped");
}

#[test]
fn ensure_session_creates_when_round2_pending_today() {
    // round1 done today but round2 not yet attempted → still need to
    // pop so the user can finish today's second round.
    let mut b = MistakeBook::default();
    let mut entry = entry_for(drill_a(), now());
    entry.today = Some(TodayAttempts {
        date: d("2026-04-27"),
        round1: Some(true),
        round2: None,
    });
    b.entries.push(entry);
    let started = b.ensure_session(d("2026-04-27"));
    assert!(started);
}

#[test]
fn ensure_session_creates_when_today_is_yesterdays_data() {
    // entry.today carries yesterday's two-round verdicts — today is a
    // fresh day, must pop.
    let mut b = MistakeBook::default();
    let mut entry = entry_for(drill_a(), now());
    entry.today = Some(TodayAttempts {
        date: d("2026-04-26"),
        round1: Some(true),
        round2: Some(true),
    });
    b.entries.push(entry);
    let started = b.ensure_session(d("2026-04-27"));
    assert!(started);
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

#[test]
fn purge_course_removes_from_wrong_streaks_entries_and_session_queue() {
    let mut b = MistakeBook::default();
    b.wrong_streaks.insert(drill_key(&drill_a()), 1);
    b.wrong_streaks.insert(drill_key(&drill_b()), 1);
    b.entries.push(entry_for(drill_a(), now()));
    b.entries.push(entry_for(drill_b(), now()));
    b.session = Some(SessionState {
        started_on: d("2026-04-27"),
        queue: vec![drill_a(), drill_b()],
        current_round: 1,
        next_index: 1, // pointing at drill_b
        round1_completed: false,
    });
    b.purge_course("course-a");
    assert!(b.wrong_streaks.contains_key(&drill_key(&drill_b())));
    assert!(!b.wrong_streaks.contains_key(&drill_key(&drill_a())));
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].drill, drill_b());
    let s = b.session.as_ref().unwrap();
    assert_eq!(s.queue, vec![drill_b()]);
    // next_index was 1 (pointing at drill_b, idx 1); after removing
    // drill_a (idx 0), drill_b is now at idx 0, so next_index should be 0.
    assert_eq!(s.next_index, 0);
}

#[test]
fn purge_course_clears_session_when_queue_becomes_empty() {
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now()));
    b.session = Some(SessionState {
        started_on: d("2026-04-27"),
        queue: vec![drill_a()],
        current_round: 1,
        next_index: 0,
        round1_completed: false,
    });
    b.purge_course("course-a");
    assert!(b.session.is_none());
}

#[test]
fn prune_orphans_drops_entries_for_unknown_courses_or_stages() {
    use crate::storage::course::{Course, Drill, Focus, Sentence, Source, SourceKind};

    let course = Course {
        schema_version: 2,
        id: "course-a".into(),
        title: "t".into(),
        description: None,
        source: Source {
            kind: SourceKind::Manual,
            url: String::new(),
            created_at: now(),
            model: "m".into(),
        },
        sentences: vec![Sentence {
            order: 1,
            drills: vec![Drill {
                stage: 2,
                focus: Focus::Full,
                chinese: "x".into(),
                english: "x".into(),
                soundmark: String::new(),
            }],
        }],
    };
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now())); // course-a / s1 / d2 → exists
    b.entries.push(entry_for(drill_b(), now())); // course-b → unknown course
    b.entries.push(entry_for(
        DrillRef {
            course_id: "course-a".into(),
            sentence_order: 1,
            drill_stage: 99, // unknown stage
        },
        now(),
    ));

    b.prune_orphans(|id| {
        if id == "course-a" {
            Some(&course)
        } else {
            None
        }
    });
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].drill, drill_a());
}

#[test]
fn prune_orphans_also_prunes_session_queue_and_adjusts_next_index() {
    use crate::storage::course::{Course, Drill, Focus, Sentence, Source, SourceKind};
    let course_b_only = Course {
        schema_version: 2,
        id: "course-b".into(),
        title: "t".into(),
        description: None,
        source: Source {
            kind: SourceKind::Manual,
            url: String::new(),
            created_at: now(),
            model: "m".into(),
        },
        sentences: vec![Sentence {
            order: 2,
            drills: vec![Drill {
                stage: 1,
                focus: Focus::Full,
                chinese: "x".into(),
                english: "x".into(),
                soundmark: String::new(),
            }],
        }],
    };
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now())); // course-a → orphan
    b.entries.push(entry_for(drill_b(), now())); // course-b s2 d1 → live
    b.session = Some(SessionState {
        started_on: d("2026-04-27"),
        queue: vec![drill_a(), drill_b()],
        current_round: 1,
        next_index: 1, // pointing at drill_b
        round1_completed: false,
    });
    b.prune_orphans(|id| {
        if id == "course-b" {
            Some(&course_b_only)
        } else {
            None
        }
    });
    let s = b.session.as_ref().unwrap();
    assert_eq!(s.queue, vec![drill_b()]);
    assert_eq!(s.next_index, 0);
    assert_eq!(b.entries.len(), 1);
}

#[test]
fn prune_orphans_clears_session_when_all_drills_orphaned() {
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now()));
    b.session = Some(SessionState {
        started_on: d("2026-04-27"),
        queue: vec![drill_a()],
        current_round: 1,
        next_index: 0,
        round1_completed: false,
    });
    // No course exists for any id.
    b.prune_orphans(|_| None);
    assert!(b.entries.is_empty());
    assert!(b.session.is_none());
}
