//! Integration tests for the /list course-list overlay and switch flow.

use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use inkworm::app::{App, Screen};
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::{load_course, save_course};
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::tts::speaker::{NullSpeaker, Speaker};
use inkworm::ui::course_list::{CourseView, OVER_LEARNED_THRESHOLD};
use tokio::sync::mpsc;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn seed_two_courses(paths: &DataPaths) {
    let base = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    for (id, date) in [
        ("2026-04-10-course-a", "2026-04-10T00:00:00Z"),
        ("2026-04-20-course-b", "2026-04-20T00:00:00Z"),
    ] {
        let mut v: serde_json::Value = serde_json::from_str(&base).unwrap();
        v["id"] = serde_json::Value::String(id.into());
        v["source"]["createdAt"] = serde_json::Value::String(date.into());
        let course: inkworm::storage::course::Course = serde_json::from_value(v).unwrap();
        save_course(&paths.courses_dir, &course).unwrap();
    }
}

fn make_app(paths: DataPaths, progress: Progress) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    let active_id = progress.active_course_id.clone();
    let course = active_id
        .as_deref()
        .and_then(|id| load_course(&paths.courses_dir, id).ok());
    let speaker: Arc<dyn Speaker> = Arc::new(NullSpeaker);
    let bundle_player = std::sync::Arc::new(inkworm::audio::player::BundlePlayer::new(None));
    App::new(
        course,
        progress,
        paths,
        Arc::new(SystemClock),
        Config::default(),
        inkworm::storage::mistakes::MistakeBook::empty(),
        None,
        task_tx,
        speaker,
        bundle_player,
    )
}

#[test]
fn list_command_opens_overlay_and_sorts_newest_first() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    seed_two_courses(&paths);

    let mut app = make_app(paths, Progress::empty());

    // Ctrl+P, then "list", then Enter.
    app.on_input(ctrl('p'));
    for c in "list".chars() {
        app.on_input(key(KeyCode::Char(c)));
    }
    app.on_input(key(KeyCode::Enter));

    assert!(matches!(app.screen, Screen::CourseList));
    let state = app.course_list.as_ref().unwrap();
    assert_eq!(state.items.len(), 2);
    assert_eq!(state.items[0].meta.id, "2026-04-20-course-b"); // newest first
    assert_eq!(state.items[1].meta.id, "2026-04-10-course-a");
}

#[test]
fn tab_switches_course_list_from_active_to_mastered_view() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    seed_two_courses(&paths);

    let mut progress = Progress::empty();
    progress.course_mut("2026-04-10-course-a").completion_count = OVER_LEARNED_THRESHOLD;
    let mut app = make_app(paths, progress);

    app.open_course_list();

    let state = app.course_list.as_ref().unwrap();
    assert_eq!(state.view, CourseView::Active);
    assert_eq!(
        state.selected_item().unwrap().meta.id,
        "2026-04-20-course-b"
    );

    app.on_input(key(KeyCode::Tab));

    let state = app.course_list.as_ref().unwrap();
    assert_eq!(state.view, CourseView::Mastered);
    assert_eq!(
        state.selected_item().unwrap().meta.id,
        "2026-04-10-course-a"
    );
}

#[test]
fn arrow_keys_wrap_within_mastered_view() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    seed_two_courses(&paths);

    let mut progress = Progress::empty();
    for (id, studied_at) in [
        ("2026-04-10-course-a", "2026-04-01T00:00:00Z"),
        ("2026-04-20-course-b", "2026-04-02T00:00:00Z"),
    ] {
        let cp = progress.course_mut(id);
        cp.completion_count = OVER_LEARNED_THRESHOLD;
        cp.last_studied_at = studied_at.parse().unwrap();
    }
    let mut app = make_app(paths, progress);
    app.open_course_list();

    let state = app.course_list.as_ref().unwrap();
    assert_eq!(state.view, CourseView::Mastered);
    assert_eq!(
        state.selected_item().unwrap().meta.id,
        "2026-04-10-course-a"
    );

    app.on_input(key(KeyCode::Up));
    assert_eq!(
        app.course_list
            .as_ref()
            .unwrap()
            .selected_item()
            .unwrap()
            .meta
            .id,
        "2026-04-20-course-b"
    );

    app.on_input(key(KeyCode::Down));
    assert_eq!(
        app.course_list
            .as_ref()
            .unwrap()
            .selected_item()
            .unwrap()
            .meta
            .id,
        "2026-04-10-course-a"
    );
}

#[tokio::test]
async fn switch_course_updates_active_and_returns_to_study() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    seed_two_courses(&paths);

    // No active course: list opens at index 0 (course-b, newest first).
    // Down moves selection to index 1 (course-a); Enter switches to course-a.
    let mut app = make_app(paths.clone(), Progress::empty());

    app.open_course_list();
    app.on_input(key(KeyCode::Down));
    app.on_input(key(KeyCode::Enter));

    assert!(matches!(app.screen, Screen::Study));
    assert_eq!(
        app.study.progress().active_course_id.as_deref(),
        Some("2026-04-10-course-a")
    );
    // Progress file on disk reflects the switch.
    let reloaded = Progress::load(&paths.progress_file).unwrap();
    assert_eq!(
        reloaded.active_course_id.as_deref(),
        Some("2026-04-10-course-a")
    );
}

#[tokio::test]
async fn over_learned_relearn_confirms_then_starts_full_only_review() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    seed_two_courses(&paths);
    let course = load_course(&paths.courses_dir, "2026-04-10-course-a").unwrap();

    let mut progress = Progress::empty();
    progress.active_course_id = Some(course.id.clone());
    let cp = progress.course_mut(&course.id);
    cp.completion_count = OVER_LEARNED_THRESHOLD;
    for sentence in &course.sentences {
        let sp = cp.sentences.entry(sentence.order.to_string()).or_default();
        for drill in &sentence.drills {
            sp.drills
                .entry(drill.stage.to_string())
                .or_default()
                .mastered_count = 1;
        }
    }
    let mut app = make_app(paths.clone(), progress);

    assert!(matches!(app.screen, Screen::CourseList));
    assert_eq!(app.course_list.as_ref().unwrap().view, CourseView::Mastered);

    // First Enter only arms the confirmation and must not mutate progress.
    app.on_input(key(KeyCode::Enter));
    assert!(matches!(app.screen, Screen::CourseList));
    let first_sentence = &course.sentences[0];
    let first_full = first_sentence.drills.last().unwrap();
    assert_eq!(
        app.study.progress().courses[&course.id].sentences[&first_sentence.order.to_string()]
            .drills[&first_full.stage.to_string()]
            .mastered_count,
        1
    );

    // Second Enter confirms, resets only final drills, and starts at full.
    app.on_input(key(KeyCode::Enter));
    assert!(matches!(app.screen, Screen::Study));
    assert!(app.study.is_full_only_review());
    assert_eq!(app.study.current_drill().unwrap().stage, first_full.stage);
    let cp = app.study.progress().course(&course.id).unwrap();
    for sentence in &course.sentences {
        let last_stage = sentence.drills.last().unwrap().stage;
        for drill in &sentence.drills {
            let mastered = cp.sentences[&sentence.order.to_string()].drills
                [&drill.stage.to_string()]
                .mastered_count;
            if drill.stage == last_stage {
                assert_eq!(mastered, 0, "full drill should restart");
            } else {
                assert_eq!(mastered, 1, "progressive drill should stay mastered");
            }
        }
    }

    let saved = Progress::load(&paths.progress_file).unwrap();
    assert_eq!(
        saved.courses[&course.id].completion_count,
        OVER_LEARNED_THRESHOLD
    );

    // Re-selecting during an unfinished full-only pass resumes instead of
    // clearing the full sentences already completed in this pass.
    for c in first_full.english.chars() {
        app.on_input(key(KeyCode::Char(c)));
    }
    app.on_input(key(KeyCode::Enter));
    assert_eq!(
        app.study.current_sentence().unwrap().order,
        course.sentences[1].order
    );
    app.open_course_list();
    app.on_input(key(KeyCode::Enter));
    app.on_input(key(KeyCode::Enter));
    assert!(matches!(app.screen, Screen::Study));
    assert_eq!(
        app.study.current_sentence().unwrap().order,
        course.sentences[1].order
    );
    assert_eq!(
        app.study.progress().courses[&course.id].sentences[&first_sentence.order.to_string()]
            .drills[&first_full.stage.to_string()]
            .mastered_count,
        1
    );
}

#[test]
fn esc_closes_list_without_changing_active() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    seed_two_courses(&paths);

    let mut progress = Progress::empty();
    progress.active_course_id = Some("2026-04-10-course-a".into());
    let mut app = make_app(paths, progress);

    app.open_course_list();
    app.on_input(key(KeyCode::Down));
    app.on_input(key(KeyCode::Esc));

    assert!(matches!(app.screen, Screen::Study));
    assert_eq!(
        app.study.progress().active_course_id.as_deref(),
        Some("2026-04-10-course-a")
    );
}

#[test]
fn empty_list_shows_overlay_without_panicking() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let mut app = make_app(paths, Progress::empty());
    app.open_course_list();

    assert!(matches!(app.screen, Screen::CourseList));
    let state = app.course_list.as_ref().unwrap();
    assert!(state.is_empty());
}
