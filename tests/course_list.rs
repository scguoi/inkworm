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
