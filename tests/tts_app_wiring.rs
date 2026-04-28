//! Integration tests for App ↔ Speaker wiring.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use inkworm::app::App;
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::{load_course, save_course, Course};
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::tts::speaker::{Speaker, TtsError};
use tokio::sync::mpsc;

struct MockSpeaker {
    spoken: Arc<Mutex<Vec<String>>>,
    cancels: Arc<AtomicUsize>,
}

impl MockSpeaker {
    fn new() -> (Arc<Self>, Arc<Mutex<Vec<String>>>, Arc<AtomicUsize>) {
        let spoken = Arc::new(Mutex::new(Vec::<String>::new()));
        let cancels = Arc::new(AtomicUsize::new(0));
        let mock = Arc::new(Self {
            spoken: Arc::clone(&spoken),
            cancels: Arc::clone(&cancels),
        });
        (mock, spoken, cancels)
    }
}

#[async_trait]
impl Speaker for MockSpeaker {
    async fn speak(&self, text: &str) -> Result<(), TtsError> {
        self.spoken.lock().unwrap().push(text.to_string());
        Ok(())
    }
    fn cancel(&self) {
        self.cancels.fetch_add(1, Ordering::SeqCst);
    }
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn seed_one_course(paths: &DataPaths) -> Course {
    let base = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    let course: Course = serde_json::from_str(&base).unwrap();
    save_course(&paths.courses_dir, &course).unwrap();
    course
}

fn make_app(paths: DataPaths, speaker: Arc<dyn Speaker>, course: Option<Course>) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    let mut progress = Progress::empty();
    if let Some(c) = &course {
        progress.active_course_id = Some(c.id.clone());
    }
    // Force TTS on + fill creds so speak_current_drill's should_speak gate
    // passes regardless of the probed audio device.
    let mut config = Config::default();
    config.tts.r#override = inkworm::config::TtsOverride::On;
    config.tts.iflytek.app_id = "test-app".into();
    config.tts.iflytek.api_key = "test-key".into();
    config.tts.iflytek.api_secret = "test-secret".into();
    App::new(
        course,
        progress,
        paths,
        Arc::new(SystemClock),
        config,
        inkworm::storage::mistakes::MistakeBook::empty(),
        None,
        task_tx,
        speaker,
    )
}

async fn settle() {
    // speak_current_drill spawns a tokio task; yield so it runs.
    tokio::time::sleep(Duration::from_millis(20)).await;
}

#[tokio::test]
async fn skip_advances_drill_and_speaks_new_english() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_one_course(&paths);
    let (mock, spoken, cancels) = MockSpeaker::new();
    let mut app = make_app(paths, mock.clone(), Some(course.clone()));

    // Startup speak fires once from the initial state.
    app.speak_current_drill();
    settle().await;

    let before_count = spoken.lock().unwrap().len();

    // Tab to skip the current drill → should cancel previous + speak the next drill.
    app.on_input(key(KeyCode::Tab));
    settle().await;

    assert!(
        cancels.load(Ordering::SeqCst) >= 2,
        "at least two cancels: startup + skip"
    );
    let spoken_snapshot = spoken.lock().unwrap().clone();
    assert!(
        spoken_snapshot.len() > before_count,
        "speak was invoked after skip, got {spoken_snapshot:?}"
    );
    let expected = course.sentences[0].drills[1].english.clone();
    assert_eq!(spoken_snapshot.last().unwrap(), &expected);
}

#[tokio::test]
async fn correct_answer_then_any_key_advances_and_speaks() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_one_course(&paths);
    let (mock, spoken, _cancels) = MockSpeaker::new();
    let mut app = make_app(paths, mock.clone(), Some(course.clone()));

    let first_drill_english = course.sentences[0].drills[0].english.clone();
    for c in first_drill_english.chars() {
        app.on_input(key(KeyCode::Char(c)));
    }
    app.on_input(key(KeyCode::Enter));
    app.on_input(key(KeyCode::Char(' ')));
    settle().await;

    let spoken_snapshot = spoken.lock().unwrap().clone();
    let next_english = course.sentences[0].drills[1].english.clone();
    assert!(
        spoken_snapshot.iter().any(|s| s == &next_english),
        "expected to have spoken {:?}, got {:?}",
        next_english,
        spoken_snapshot,
    );
}

#[tokio::test]
async fn switch_to_course_speaks_new_course_first_drill() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let base = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    let mut v1: serde_json::Value = serde_json::from_str(&base).unwrap();
    v1["id"] = serde_json::Value::String("course-a".into());
    let course_a: Course = serde_json::from_value(v1).unwrap();
    save_course(&paths.courses_dir, &course_a).unwrap();

    let mut v2: serde_json::Value = serde_json::from_str(&base).unwrap();
    v2["id"] = serde_json::Value::String("course-b".into());
    v2["sentences"][0]["drills"][0]["english"] =
        serde_json::Value::String("Hello other course".into());
    let course_b: Course = serde_json::from_value(v2).unwrap();
    save_course(&paths.courses_dir, &course_b).unwrap();

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let mut app = make_app(paths.clone(), mock.clone(), Some(course_a.clone()));

    app.open_course_list();
    let list = app.course_list.as_ref().unwrap();
    let target_idx = list
        .items
        .iter()
        .position(|i| i.meta.id == "course-b")
        .unwrap();
    while app.course_list.as_ref().unwrap().selected != target_idx {
        app.on_input(key(KeyCode::Down));
    }
    app.on_input(key(KeyCode::Enter));
    settle().await;

    let spoken_snapshot = spoken.lock().unwrap().clone();
    assert!(
        spoken_snapshot.iter().any(|s| s == "Hello other course"),
        "expected course-b first drill to have been spoken, got {spoken_snapshot:?}"
    );
    let reloaded = load_course(&paths.courses_dir, "course-b").unwrap();
    assert_eq!(reloaded.id, "course-b");
}
