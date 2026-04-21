//! Integration tests for /tts palette subcommands.

use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use inkworm::app::App;
use inkworm::clock::SystemClock;
use inkworm::config::{Config, TtsOverride};
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
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

fn make_app(paths: DataPaths) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    App::new(
        None,
        Progress::empty(),
        paths,
        Arc::new(SystemClock),
        Config::default(),
        task_tx,
    )
}

fn type_chars(app: &mut App, s: &str) {
    for c in s.chars() {
        app.on_input(key(KeyCode::Char(c)));
    }
}

#[test]
fn tts_on_updates_config_and_persists() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let mut app = make_app(paths.clone());

    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts on");
    app.on_input(key(KeyCode::Enter));

    assert_eq!(app.config.tts.r#override, TtsOverride::On);
    let reloaded = Config::load(&paths.config_file).unwrap();
    assert_eq!(reloaded.tts.r#override, TtsOverride::On);
}

#[test]
fn tts_off_then_auto_cycles_correctly() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let mut app = make_app(paths.clone());

    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts off");
    app.on_input(key(KeyCode::Enter));
    assert_eq!(app.config.tts.r#override, TtsOverride::Off);

    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts auto");
    app.on_input(key(KeyCode::Enter));
    assert_eq!(app.config.tts.r#override, TtsOverride::Auto);

    let reloaded = Config::load(&paths.config_file).unwrap();
    assert_eq!(reloaded.tts.r#override, TtsOverride::Auto);
}

#[test]
fn tts_clear_cache_removes_wav_files() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    std::fs::write(paths.tts_cache_dir.join("a.wav"), b"x").unwrap();
    std::fs::write(paths.tts_cache_dir.join("b.wav"), b"y").unwrap();
    let mut app = make_app(paths.clone());

    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts clear-cache");
    app.on_input(key(KeyCode::Enter));

    assert!(!paths.tts_cache_dir.join("a.wav").exists());
    assert!(!paths.tts_cache_dir.join("b.wav").exists());
    assert!(paths.tts_cache_dir.is_dir(), "directory itself preserved");
}

#[test]
fn tts_unknown_arg_is_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let mut app = make_app(paths);

    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts wat");
    app.on_input(key(KeyCode::Enter));

    assert_eq!(app.config.tts.r#override, TtsOverride::Auto);
}

#[test]
fn tts_tab_completes_with_trailing_space() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let mut app = make_app(paths);

    app.on_input(ctrl('p'));
    app.on_input(key(KeyCode::Char('t')));
    app.on_input(key(KeyCode::Tab));

    let palette = app.palette.as_ref().expect("palette should be open");
    assert_eq!(palette.input, "/tts ");
}
