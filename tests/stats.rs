mod common;

use chrono::{TimeZone, Utc};
use inkworm::clock::FixedClock;
use inkworm::stats::StatsTracker;
use inkworm::storage::progress::Progress;
use inkworm::storage::stats::Stats;
use inkworm::ui::study::StudyState;

fn at(h: u32, m: u32, s: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 13, h, m, s).unwrap()
}

#[test]
fn retry_until_correct_counts_each_submit() {
    let course = common::load_minimal_course();
    let english = course.sentences[0].drills[0].english.clone();
    let words = english.split_whitespace().count() as u32;

    let today = at(12, 0, 0).with_timezone(&chrono::Local).date_naive();
    let mut tr = StatsTracker::from_stats(Stats::empty(), today);

    let mut s = StudyState::new(Some(course), Progress::empty());
    let clk = FixedClock(at(12, 0, 0));

    // Wrong attempt
    for c in "wrong".chars() {
        s.type_char(c);
    }
    let (_o, tick) = s.submit(&clk);
    let t = tick.expect("tick present");
    tr.on_submit(at(12, 0, 0), t.was_correct, t.words);
    s.clear_and_restart();

    // Correct attempt
    for c in english.chars() {
        s.type_char(c);
    }
    let clk2 = FixedClock(at(12, 0, 5));
    let (_o2, tick2) = s.submit(&clk2);
    let t2 = tick2.expect("tick present");
    tr.on_submit(at(12, 0, 5), t2.was_correct, t2.words);

    let d = tr.today_stats();
    assert_eq!(d.submits, 2, "both attempts counted");
    assert_eq!(d.correct, 1, "only the second counted correct");
    assert_eq!(d.words, words * 2);
}

#[test]
fn submit_writes_stats_file_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stats.json");
    let today = chrono::Local::now().date_naive();
    let mut tr = StatsTracker::from_stats(Stats::empty(), today);
    tr.on_submit(Utc::now(), true, 3);
    tr.snapshot().save(&path).unwrap();
    let reloaded = Stats::load(&path).unwrap();
    assert!(reloaded.days.contains_key(&today));
}
