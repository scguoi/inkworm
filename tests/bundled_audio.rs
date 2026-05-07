//! Integration tests for bundled course audio.
//!
//! Strategy: same `MockSpeaker` pattern as `tts_app_wiring.rs` —
//! count `speak()` calls. When a bundled mp3 is available for the
//! current drill, the mock speaker's `speak()` must NOT be called.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use inkworm::app::App;
use inkworm::audio::player::BundlePlayer;
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::{save_course, Course};
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

fn seed_course(paths: &DataPaths) -> Course {
    let base = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    let course: Course = serde_json::from_str(&base).unwrap();
    save_course(&paths.courses_dir, &course).unwrap();
    course
}

fn make_app(paths: DataPaths, speaker: Arc<dyn Speaker>, course: Course) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    let mut progress = Progress::empty();
    progress.active_course_id = Some(course.id.clone());
    let mut config = Config::default();
    config.tts.r#override = inkworm::config::TtsOverride::On;
    config.tts.iflytek.app_id = "test-app".into();
    config.tts.iflytek.api_key = "test-key".into();
    config.tts.iflytek.api_secret = "test-secret".into();
    let bundle_player = Arc::new(BundlePlayer::new(None));
    App::new(
        Some(course),
        progress,
        paths,
        Arc::new(SystemClock),
        config,
        inkworm::storage::mistakes::MistakeBook::empty(),
        None,
        task_tx,
        speaker,
        bundle_player,
    )
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(40)).await;
}

/// Write `<courses_dir>/<yyyy-mm>/<id_tail>/s{order:02}-d{stage}.mp3`
/// using the silence fixture. Caller specifies the course id.
fn place_bundle_file(courses_dir: &std::path::Path, course_id: &str, order: u32, stage: u32) {
    assert!(course_id.len() >= 11);
    let yyyy_mm = &course_id[0..7];
    let tail = &course_id[8..];
    let dir = courses_dir.join(yyyy_mm).join(tail);
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = std::fs::read("fixtures/audio/silence.mp3").unwrap();
    std::fs::write(dir.join(format!("s{:02}-d{}.mp3", order, stage)), &bytes).unwrap();
}

#[tokio::test]
async fn bundled_hit_skips_speaker() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    // Bundle the very first drill the app will speak on startup
    let order0 = course.sentences[0].order;
    let stage0 = course.sentences[0].drills[0].stage;
    place_bundle_file(&paths.courses_dir, &course.id, order0, stage0);

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let app = make_app(paths, mock.clone(), course);
    app.speak_current_drill();
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    assert!(
        speaks.is_empty(),
        "speaker.speak must not be called when bundle is present, got {speaks:?}"
    );
}

#[tokio::test]
async fn bundled_miss_falls_through_to_speaker() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    // No bundle dir at all.

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let app = make_app(paths, mock.clone(), course.clone());
    app.speak_current_drill();
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    let expected = course.sentences[0].drills[0].english.clone();
    assert_eq!(
        speaks,
        vec![expected],
        "expected one fall-through speak call"
    );
}

#[tokio::test]
async fn bundled_partial_miss_falls_through_for_missing_drill() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    // Place a bundle for s01-d1 but NOT for s01-d2.
    let order0 = course.sentences[0].order;
    let stage0 = course.sentences[0].drills[0].stage;
    place_bundle_file(&paths.courses_dir, &course.id, order0, stage0);

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let mut app = make_app(paths, mock.clone(), course.clone());
    // Startup speak: bundle hit for s01-d1.
    app.speak_current_drill();
    settle().await;
    assert!(
        spoken.lock().unwrap().is_empty(),
        "startup should hit bundle"
    );

    // Skip to s01-d2 via Tab.
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    let tab = Event::Key(KeyEvent {
        code: KeyCode::Tab,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    app.on_input(tab);
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    let expected = course.sentences[0].drills[1].english.clone();
    assert_eq!(
        speaks,
        vec![expected],
        "fall-through expected for missing drill"
    );
}

#[tokio::test]
async fn corrupt_bundle_does_not_call_speaker() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    // Place a zero-byte mp3 — bundle "exists" so we commit to that path,
    // then decode fails and we accept silence (per spec §7).
    let order0 = course.sentences[0].order;
    let stage0 = course.sentences[0].drills[0].stage;
    let yyyy_mm = &course.id[0..7];
    let tail = &course.id[8..];
    let dir = paths.courses_dir.join(yyyy_mm).join(tail);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("s{:02}-d{}.mp3", order0, stage0)), b"").unwrap();

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let app = make_app(paths, mock.clone(), course);
    app.speak_current_drill();
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    assert!(
        speaks.is_empty(),
        "corrupt bundle must not fall through (spec §7), got {speaks:?}"
    );
}

#[tokio::test]
async fn bundled_hit_works_when_tts_session_disabled() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    let order0 = course.sentences[0].order;
    let stage0 = course.sentences[0].drills[0].stage;
    place_bundle_file(&paths.courses_dir, &course.id, order0, stage0);

    let (mock, spoken, _cancels) = MockSpeaker::new();
    let mut app = make_app(paths, mock.clone(), course);
    // Disabling the TTS session must not block the bundle path (spec §3).
    app.tts_session_disabled = true;
    app.speak_current_drill();
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    assert!(
        speaks.is_empty(),
        "bundle must play even with TTS session disabled, got {speaks:?}"
    );
}

#[tokio::test]
async fn bundled_hit_works_when_no_iflytek_creds() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let course = seed_course(&paths);
    let order0 = course.sentences[0].order;
    let stage0 = course.sentences[0].drills[0].stage;
    place_bundle_file(&paths.courses_dir, &course.id, order0, stage0);

    // Build the App with empty iFlytek creds; bundle must still play.
    let (task_tx, _task_rx) = mpsc::channel(16);
    let mut progress = inkworm::storage::progress::Progress::empty();
    progress.active_course_id = Some(course.id.clone());
    let mut config = inkworm::config::Config::default();
    config.tts.r#override = inkworm::config::TtsOverride::On;
    // (creds left empty)
    let bundle_player = Arc::new(BundlePlayer::new(None));
    let (mock, spoken, _cancels) = MockSpeaker::new();
    let speaker: Arc<dyn Speaker> = mock;
    let app = App::new(
        Some(course),
        progress,
        paths,
        Arc::new(SystemClock),
        config,
        inkworm::storage::mistakes::MistakeBook::empty(),
        None,
        task_tx,
        speaker,
        bundle_player,
    );
    app.speak_current_drill();
    settle().await;

    let speaks = spoken.lock().unwrap().clone();
    assert!(
        speaks.is_empty(),
        "bundle must play with empty iFlytek creds, got {speaks:?}"
    );
}
