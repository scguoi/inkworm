use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, TimeZone, Utc};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use inkworm::app::{App, Screen};
use inkworm::clock::Clock;
use inkworm::config::Config;
use inkworm::storage::course::{save_course, Course};
use inkworm::storage::mistakes::{DrillRef, MistakeBook, MistakeEntry, SessionState};
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::tts::speaker::NullSpeaker;
use inkworm::ui::study::{FeedbackState, StudyMode};

struct TestClock(Mutex<DateTime<Utc>>);
impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

struct Harness {
    _tmp: tempfile::TempDir,
    paths: DataPaths,
    clock: Arc<TestClock>,
}
impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        Self {
            paths: DataPaths::for_tests(tmp.path().to_owned()),
            _tmp: tmp,
            clock: Arc::new(TestClock(Mutex::new(
                Utc.with_ymd_and_hms(2026, 10, 10, 4, 0, 0).unwrap(),
            ))),
        }
    }

    fn seed(&self, progress: &mut Progress, name: &str, age: Duration, passes: u32) -> Course {
        let mut course: Course =
            serde_json::from_str(include_str!("../fixtures/courses/good/minimal.json")).unwrap();
        course.id = format!("2026-04-21-{name}");
        save_course(&self.paths.courses_dir, &course).unwrap();
        let cp = progress.course_mut(&course.id);
        cp.last_studied_at = self.clock.now() - age;
        cp.completion_count = passes;
        for sentence in &course.sentences {
            for drill in &sentence.drills {
                cp.sentences
                    .entry(sentence.order.to_string())
                    .or_default()
                    .drills
                    .entry(drill.stage.to_string())
                    .or_default()
                    .mastered_count = 1;
            }
        }
        course
    }

    fn app(&self, progress: Progress, mistakes: MistakeBook) -> App {
        let course = progress
            .active_course_id
            .as_ref()
            .and_then(|id| inkworm::storage::course::load_course(&self.paths.courses_dir, id).ok());
        App::new(
            course,
            progress,
            self.paths.clone(),
            self.clock.clone(),
            Config::default(),
            mistakes,
            None,
            tokio::sync::mpsc::channel(32).0,
            Arc::new(NullSpeaker),
            Arc::new(inkworm::audio::player::BundlePlayer::new(None)),
        )
    }

    fn reload(&self) -> Progress {
        Progress::load(&self.paths.progress_file).unwrap()
    }

    fn elapse(&self, duration: Duration) {
        *self.clock.0.lock().unwrap() += duration;
    }
}

fn key(app: &mut App, code: KeyCode) {
    app.on_input(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}
fn command(app: &mut App, text: &str) {
    app.on_input(Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )));
    for c in text.chars() {
        key(app, KeyCode::Char(c));
    }
    key(app, KeyCode::Enter);
}
fn answer(app: &mut App) {
    let english = app.study.current_drill().unwrap().english.clone();
    for c in english.chars() {
        app.study.type_char(c);
    }
    key(app, KeyCode::Enter);
    assert_eq!(app.study.feedback(), &FeedbackState::Correct);
}
fn finish_course(app: &mut App) {
    let count = app.study.current_course().unwrap().sentences.len();
    for _ in 0..count {
        answer(app);
        key(app, KeyCode::Enter);
    }
}
fn one_mistake(course: &Course, clock: &dyn Clock) -> MistakeBook {
    let drill = DrillRef {
        course_id: course.id.clone(),
        sentence_order: course.sentences[0].order,
        drill_stage: course.sentences[0].drills[0].stage,
    };
    let mut book = MistakeBook::empty();
    book.entries.push(MistakeEntry {
        drill: drill.clone(),
        entered_at: clock.now(),
        streak_days: 0,
        last_qualified_date: None,
        today: None,
    });
    book.session = Some(SessionState {
        started_on: clock.today_local(),
        queue: vec![drill],
        current_round: 2,
        next_index: 0,
        round1_completed: true,
    });
    book
}

#[test]
fn selects_three_oldest_mastered_courses_with_strict_thirty_day_cutoff() {
    let h = Harness::new();
    let mut p = Progress::empty();
    h.seed(&mut p, "not-mastered", Duration::days(100), 3);
    h.seed(&mut p, "boundary", Duration::days(30), 4);
    h.seed(&mut p, "recent", Duration::days(29), 4);
    h.seed(
        &mut p,
        "fourth",
        Duration::days(30) + Duration::seconds(1),
        4,
    );
    let third = h.seed(&mut p, "third", Duration::days(40), 4);
    let first = h.seed(&mut p, "first", Duration::days(60), 4);
    let second = h.seed(&mut p, "second", Duration::days(50), 5);
    let app = h.app(p, MistakeBook::empty());
    let review = app.study.progress().course_review.as_ref().unwrap();
    assert_eq!(review.queue, vec![first.id.clone(), second.id, third.id]);
    assert_eq!(app.study.current_course().unwrap().id, first.id);
    assert!(app.study.is_full_only_review());
    assert_eq!(app.study.current_drill().unwrap().stage, 3);
    assert!(matches!(app.screen, Screen::Study));
}

#[test]
fn mistakes_take_priority_and_completion_starts_course_review() {
    let h = Harness::new();
    let mut p = Progress::empty();
    let course = h.seed(&mut p, "old", Duration::days(40), 4);
    let mistakes = one_mistake(&course, h.clock.as_ref());
    let mut app = h.app(p, mistakes);
    assert_eq!(app.study.mode(), &StudyMode::Mistakes);
    answer(&mut app);
    h.elapse(Duration::milliseconds(600));
    app.on_tick();
    assert_eq!(app.study.mode(), &StudyMode::Course);
    assert!(app.study.is_full_only_review());
    assert_eq!(app.study.current_course().unwrap().id, course.id);
    assert!(app.study.progress().course_review.is_some());
}

#[test]
fn finishing_three_courses_opens_list_and_does_not_schedule_more_that_day() {
    let h = Harness::new();
    let mut p = Progress::empty();
    for (name, age) in [("a", 60), ("b", 50), ("c", 40), ("d", 35)] {
        h.seed(&mut p, name, Duration::days(age), 4);
    }
    let mut app = h.app(p, MistakeBook::empty());
    for i in 0..3 {
        assert_eq!(
            app.study
                .progress()
                .course_review
                .as_ref()
                .unwrap()
                .next_index,
            i
        );
        finish_course(&mut app);
    }
    assert!(matches!(app.screen, Screen::CourseList));
    let saved = h.reload();
    for name in ["a", "b", "c"] {
        assert_eq!(
            saved
                .course(&format!("2026-04-21-{name}"))
                .unwrap()
                .completion_count,
            5
        );
    }
    let restarted = h.app(saved, MistakeBook::empty());
    assert!(restarted
        .study
        .progress()
        .course_review
        .as_ref()
        .unwrap()
        .current_id()
        .is_none());
    h.elapse(Duration::days(1));
    let next_day = h.app(h.reload(), MistakeBook::empty());
    assert_eq!(next_day.study.current_course().unwrap().id, "2026-04-21-d");
}

#[test]
fn partial_review_survives_restart_even_after_last_studied_changes() {
    let h = Harness::new();
    let mut p = Progress::empty();
    let course = h.seed(&mut p, "old", Duration::days(40), 4);
    let mut app = h.app(p, MistakeBook::empty());
    answer(&mut app);
    key(&mut app, KeyCode::Enter);
    command(&mut app, "/quit");
    assert!(app.should_quit);
    let mut restarted = h.app(h.reload(), MistakeBook::empty());
    assert_eq!(restarted.study.current_sentence().unwrap().order, 2);
    assert_eq!(restarted.study.current_course().unwrap().id, course.id);
    for _ in 1..course.sentences.len() {
        answer(&mut restarted);
        key(&mut restarted, KeyCode::Enter);
    }
    assert!(matches!(restarted.screen, Screen::CourseList));
}

#[test]
fn quitting_after_last_correct_answer_counts_course_once_on_restart() {
    let h = Harness::new();
    let mut p = Progress::empty();
    let course = h.seed(&mut p, "old", Duration::days(40), 4);
    let mut app = h.app(p, MistakeBook::empty());
    for _ in 1..course.sentences.len() {
        answer(&mut app);
        key(&mut app, KeyCode::Enter);
    }
    answer(&mut app);
    command(&mut app, "/quit");
    let app = h.app(h.reload(), MistakeBook::empty());
    assert!(matches!(app.screen, Screen::CourseList));
    assert_eq!(
        app.study
            .progress()
            .course(&course.id)
            .unwrap()
            .completion_count,
        5
    );
    let app = h.app(h.reload(), MistakeBook::empty());
    assert_eq!(
        app.study
            .progress()
            .course(&course.id)
            .unwrap()
            .completion_count,
        5
    );
}

#[test]
fn review_blocks_skip_switch_import_delete_but_allows_quit() {
    let h = Harness::new();
    let mut p = Progress::empty();
    let course = h.seed(&mut p, "old", Duration::days(40), 4);
    let mut app = h.app(p, MistakeBook::empty());
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Esc);
    for cmd in ["/skip", "/list", "/import", "/delete"] {
        command(&mut app, cmd);
        assert!(matches!(app.screen, Screen::Study), "{cmd}");
        assert_eq!(app.study.current_sentence().unwrap().order, 1);
        assert_eq!(app.study.current_course().unwrap().id, course.id);
    }
    answer(&mut app);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.study.current_sentence().unwrap().order, 1);
    command(&mut app, "/quit");
    assert!(app.should_quit);
}

#[test]
fn missing_queued_course_does_not_block_remaining_course() {
    let h = Harness::new();
    let mut p = Progress::empty();
    let first = h.seed(&mut p, "first", Duration::days(50), 4);
    let second = h.seed(&mut p, "second", Duration::days(40), 4);
    let _app = h.app(p, MistakeBook::empty());
    inkworm::storage::course::delete_course(&h.paths.courses_dir, &first.id).unwrap();
    let app = h.app(h.reload(), MistakeBook::empty());
    assert_eq!(app.study.current_course().unwrap().id, second.id);
}

#[test]
fn carried_course_survives_midnight_mistakes_with_no_more_candidates() {
    let h = Harness::new();
    let mut p = Progress::empty();
    let course = h.seed(&mut p, "first", Duration::days(50), 4);
    let mut app = h.app(p, MistakeBook::empty());
    answer(&mut app);
    key(&mut app, KeyCode::Enter);
    h.elapse(Duration::days(1));
    app.mistakes = one_mistake(&course, h.clock.as_ref());
    app.on_tick();
    assert_eq!(app.study.mode(), &StudyMode::Mistakes);
    answer(&mut app);
    h.elapse(Duration::milliseconds(600));
    app.on_tick();
    assert_eq!(app.study.mode(), &StudyMode::Course);
    assert_eq!(app.study.current_sentence().unwrap().order, 2);
    for _ in 1..course.sentences.len() {
        answer(&mut app);
        key(&mut app, KeyCode::Enter);
    }
    assert!(matches!(app.screen, Screen::CourseList));
    assert_eq!(
        h.reload().course_review.unwrap().date,
        h.clock.today_local()
    );
}

#[test]
fn old_progress_without_review_field_loads_and_no_eligible_course_is_forced() {
    let h = Harness::new();
    let mut p: Progress = serde_json::from_str(r#"{"schemaVersion":1,"courses":{}}"#).unwrap();
    assert!(p.course_review.is_none());
    let course = h.seed(&mut p, "recent", Duration::days(30), 4);
    p.active_course_id = Some(course.id);
    let app = h.app(p, MistakeBook::empty());
    assert!(matches!(app.screen, Screen::CourseList));
    assert!(app.study.progress().course_review.is_none());
}

#[test]
fn manual_advance_after_last_mistake_also_starts_course_review() {
    let h = Harness::new();
    let mut p = Progress::empty();
    let course = h.seed(&mut p, "old", Duration::days(40), 4);
    let mut app = h.app(p, one_mistake(&course, h.clock.as_ref()));
    answer(&mut app);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.study.mode(), &StudyMode::Course);
    assert!(app.study.is_full_only_review());
    assert!(app.study.progress().course_review.is_some());
}

#[test]
fn timed_advance_finishes_review_and_wrong_answers_do_not_advance() {
    let h = Harness::new();
    let mut p = Progress::empty();
    let course = h.seed(&mut p, "old", Duration::days(40), 4);
    let mut app = h.app(p, MistakeBook::empty());
    app.study.type_char('x');
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.study.feedback(), &FeedbackState::Wrong);
    h.elapse(Duration::seconds(1));
    app.on_tick();
    assert_eq!(app.study.current_sentence().unwrap().order, 1);
    key(&mut app, KeyCode::Enter);
    for _ in &course.sentences {
        answer(&mut app);
        h.elapse(Duration::milliseconds(600));
        app.on_tick();
    }
    assert!(matches!(app.screen, Screen::CourseList));
    assert_eq!(h.reload().course(&course.id).unwrap().completion_count, 5);
}

#[test]
fn status_bar_shows_daily_course_position() {
    let h = Harness::new();
    let mut p = Progress::empty();
    h.seed(&mut p, "old", Duration::days(40), 4);
    let app = h.app(p, MistakeBook::empty());
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let content: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(content.contains("Daily review 1/1"));
}

#[test]
fn carried_course_counts_today_and_two_more_are_added_across_restart() {
    let h = Harness::new();
    let mut p = Progress::empty();
    for (name, age) in [
        ("a", 80),
        ("b", 70),
        ("c", 60),
        ("d", 50),
        ("e", 40),
        ("f", 35),
    ] {
        h.seed(&mut p, name, Duration::days(age), 4);
    }
    let mut app = h.app(p, MistakeBook::empty());
    finish_course(&mut app);
    finish_course(&mut app);
    answer(&mut app);
    key(&mut app, KeyCode::Enter);
    command(&mut app, "/quit");
    h.elapse(Duration::days(1));
    let mut app = h.app(h.reload(), MistakeBook::empty());
    assert_eq!(app.study.current_sentence().unwrap().order, 2);
    let review = app.study.progress().course_review.as_ref().unwrap();
    assert_eq!(review.completed_today, Some(0));
    assert_eq!(
        review.queue,
        ["2026-04-21-c", "2026-04-21-d", "2026-04-21-e"]
    );
    let remaining = app.study.current_course().unwrap().sentences.len() - 1;
    for _ in 0..remaining {
        answer(&mut app);
        key(&mut app, KeyCode::Enter);
    }
    assert_eq!(
        app.study
            .progress()
            .course_review
            .as_ref()
            .unwrap()
            .completed_today,
        Some(1)
    );
    command(&mut app, "/quit");
    let mut app = h.app(h.reload(), MistakeBook::empty());
    for name in ["d", "e"] {
        assert_eq!(
            app.study.current_course().unwrap().id,
            format!("2026-04-21-{name}")
        );
        finish_course(&mut app);
    }
    assert!(matches!(app.screen, Screen::CourseList));
    assert_eq!(h.reload().course_review.unwrap().completed_today, Some(3));
    let app = h.app(h.reload(), MistakeBook::empty());
    assert!(app
        .study
        .progress()
        .course_review
        .as_ref()
        .unwrap()
        .current_id()
        .is_none());
}

#[test]
fn legacy_completed_batch_recovers_only_today_and_tops_up_available_courses() {
    let h = Harness::new();
    let mut p = Progress::empty();
    for (name, age) in [("a", 80), ("b", 70), ("c", 60), ("d", 50)] {
        h.seed(&mut p, name, Duration::days(age), 4);
    }
    let mut app = h.app(p, MistakeBook::empty());
    for _ in 0..3 {
        finish_course(&mut app);
    }
    let mut legacy = serde_json::to_value(h.reload()).unwrap();
    legacy["courseReview"]
        .as_object_mut()
        .unwrap()
        .remove("completedToday");
    let mut p: Progress = serde_json::from_value(legacy).unwrap();
    for name in ["a", "b"] {
        p.course_mut(&format!("2026-04-21-{name}")).last_studied_at -= Duration::days(1);
    }
    let mut app = h.app(p, MistakeBook::empty());
    assert_eq!(app.study.current_course().unwrap().id, "2026-04-21-d");
    assert_eq!(
        app.study
            .progress()
            .course_review
            .as_ref()
            .unwrap()
            .completed_today,
        Some(1)
    );
    finish_course(&mut app);
    assert!(matches!(app.screen, Screen::CourseList));
    assert_eq!(h.reload().course_review.unwrap().completed_today, Some(2));
}

#[test]
fn missing_course_is_replaced_without_counting_as_completed() {
    let h = Harness::new();
    let mut p = Progress::empty();
    for (name, age) in [("a", 80), ("b", 70), ("c", 60), ("d", 50)] {
        h.seed(&mut p, name, Duration::days(age), 4);
    }
    let _app = h.app(p, MistakeBook::empty());
    inkworm::storage::course::delete_course(&h.paths.courses_dir, "2026-04-21-a").unwrap();
    let mut app = h.app(h.reload(), MistakeBook::empty());
    for name in ["b", "c", "d"] {
        assert_eq!(
            app.study.current_course().unwrap().id,
            format!("2026-04-21-{name}")
        );
        finish_course(&mut app);
    }
    assert_eq!(h.reload().course_review.unwrap().completed_today, Some(3));
    assert!(matches!(app.screen, Screen::CourseList));
}

#[test]
fn last_answer_yesterday_is_not_counted_today_when_resuming_before_advance() {
    let h = Harness::new();
    let mut p = Progress::empty();
    let first = h.seed(&mut p, "a", Duration::days(80), 4);
    for (name, age) in [("b", 70), ("c", 60), ("d", 50)] {
        h.seed(&mut p, name, Duration::days(age), 4);
    }
    let mut app = h.app(p, MistakeBook::empty());
    for _ in 1..first.sentences.len() {
        answer(&mut app);
        key(&mut app, KeyCode::Enter);
    }
    answer(&mut app);
    command(&mut app, "/quit");
    h.elapse(Duration::days(1));
    let mut app = h.app(h.reload(), MistakeBook::empty());
    assert_eq!(
        app.study
            .progress()
            .course_review
            .as_ref()
            .unwrap()
            .completed_today,
        Some(0)
    );
    for name in ["b", "c", "d"] {
        assert_eq!(
            app.study.current_course().unwrap().id,
            format!("2026-04-21-{name}")
        );
        finish_course(&mut app);
    }
    assert_eq!(h.reload().course(&first.id).unwrap().completion_count, 5);
    assert_eq!(h.reload().course_review.unwrap().completed_today, Some(3));
    assert!(matches!(app.screen, Screen::CourseList));
}
