mod common;

use chrono::{TimeZone, Utc};
use inkworm::clock::FixedClock;
use inkworm::storage::progress::Progress;
use inkworm::ui::skeleton::skeleton;
use inkworm::ui::study::{FeedbackState, StudyPhase, StudyState};

fn clock() -> FixedClock {
    FixedClock(Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap())
}

#[test]
fn full_drill_cycle_persists_progress() {
    let clk = clock();
    let course = common::load_minimal_course();
    let mut state = StudyState::new(Some(course), Progress::empty());

    let english = state.current_drill().unwrap().english.clone();
    for c in english.chars() {
        state.type_char(c);
    }
    state.submit(&clk);
    assert_eq!(*state.feedback(), FeedbackState::Correct);
    state.advance();

    let p = state.progress();
    let dp = &p.courses["2026-04-21-ted-ai"].sentences["1"].drills["1"];
    assert_eq!(dp.mastered_count, 1);
    assert!(dp.last_correct_at.is_some());
}

#[test]
fn wrong_then_correct_flow() {
    let clk = clock();
    let course = common::load_minimal_course();
    let mut state = StudyState::new(Some(course), Progress::empty());

    for c in "wrong answer".chars() {
        state.type_char(c);
    }
    state.submit(&clk);
    assert_eq!(*state.feedback(), FeedbackState::Wrong);

    while !state.input().is_empty() {
        state.backspace();
    }
    let english = state.current_drill().unwrap().english.clone();
    for c in english.chars() {
        state.type_char(c);
    }
    state.submit(&clk);
    assert_eq!(*state.feedback(), FeedbackState::Correct);
}

#[test]
fn skip_then_advance_covers_all_drills() {
    let course = common::load_minimal_course();
    let total: usize = course.sentences.iter().map(|s| s.drills.len()).sum();
    let mut state = StudyState::new(Some(course), Progress::empty());

    for _ in 0..total {
        assert_eq!(*state.phase(), StudyPhase::Active);
        state.skip();
    }
    assert_eq!(*state.phase(), StudyPhase::Complete);
}

#[test]
fn palette_execute_skip() {
    let course = common::load_minimal_course();
    let mut state = StudyState::new(Some(course), Progress::empty());
    let first_drill = state.current_drill().unwrap().english.clone();

    state.skip();
    let second_drill = state.current_drill().unwrap().english.clone();
    assert_ne!(first_drill, second_drill);
}

#[test]
fn skeleton_integration() {
    let course = common::load_minimal_course();
    let drill = &course.sentences[0].drills[0];
    let skel = skeleton(&drill.english);
    assert_eq!(skel, "__ _____ ___");
}

#[test]
fn progress_persistence_round_trip() {
    let clk = clock();
    let course = common::load_minimal_course();
    let mut state = StudyState::new(Some(course.clone()), Progress::empty());

    let english = state.current_drill().unwrap().english.clone();
    for c in english.chars() {
        state.type_char(c);
    }
    state.submit(&clk);
    state.advance();

    let dir = tempfile::tempdir().unwrap();
    let progress_path = dir.path().join("progress.json");
    state.progress().save(&progress_path).unwrap();

    let reloaded = Progress::load(&progress_path).unwrap();
    let state2 = StudyState::new(Some(course), reloaded);

    assert_eq!(state2.current_drill().unwrap().stage, 2);
}
