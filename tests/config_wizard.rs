use std::sync::Arc;

use inkworm::app::{App, Screen};
use inkworm::clock::SystemClock;
use inkworm::config::{Config, IflytekConfig};
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::tts::speaker::{NullSpeaker, Speaker};
use inkworm::ui::config_wizard::{WizardOrigin, WizardStep};
use inkworm::ui::task_msg::{TaskMsg, WizardTaskMsg};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn setup(tmp: &TempDir) -> (DataPaths, Progress) {
    let paths = DataPaths::resolve(Some(tmp.path())).unwrap();
    paths.ensure_dirs().unwrap();
    let progress = Progress::load(&paths.progress_file).unwrap();
    (paths, progress)
}

fn make_app(
    paths: DataPaths,
    progress: Progress,
    cfg: Config,
) -> (App, tokio::sync::mpsc::Receiver<TaskMsg>) {
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let speaker: Arc<dyn Speaker> = Arc::new(NullSpeaker);
    let app = App::new(
        None,
        progress,
        paths,
        Arc::new(SystemClock),
        cfg,
        tx,
        speaker,
    );
    (app, rx)
}

#[tokio::test]
async fn first_run_opens_wizard() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths, progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    assert!(matches!(app.screen, Screen::ConfigWizard));
    let w = app.config_wizard.as_ref().unwrap();
    assert_eq!(w.step, WizardStep::Endpoint);
    assert_eq!(w.origin, WizardOrigin::FirstRun);
}

#[tokio::test]
async fn connectivity_ok_advances_to_tts_enable_then_saves() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role":"assistant","content":"pong"}}]
        })))
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths.clone(), progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    // Set draft fields directly and advance to Model step.
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.draft.llm.base_url = server.uri();
        w.draft.llm.api_key = "sk-test".into();
        w.draft.llm.model = "gpt-4o-mini".into();
        w.step = WizardStep::Model;
    }

    // Simulate "ConnectivityOk" arriving — now advances to TtsEnable.
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::ConnectivityOk));

    assert!(matches!(app.screen, Screen::ConfigWizard));
    let w = app.config_wizard.as_ref().unwrap();
    assert_eq!(w.step, WizardStep::TtsEnable);

    // Now at TtsEnable step with input pre-seeded to "y" (default enabled).
    // Clear and type "n" to decline TTS, then press Enter.
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)));
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)));
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    assert!(matches!(app.screen, Screen::Study));
    assert!(app.config_wizard.is_none());
    let saved = Config::load(&paths.config_file).unwrap();
    assert_eq!(saved.llm.base_url, server.uri());
    assert_eq!(saved.llm.api_key, "sk-test");
    assert_eq!(saved.llm.model, "gpt-4o-mini");
}

#[tokio::test]
async fn connectivity_failed_keeps_wizard_open_with_error() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths, progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.step = WizardStep::Model;
    }

    let err = inkworm::error::AppError::Llm(inkworm::llm::error::LlmError::Unauthorized);
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::ConnectivityFailed(err)));

    assert!(matches!(app.screen, Screen::ConfigWizard));
    let w = app.config_wizard.as_ref().unwrap();
    assert!(w.testing.is_none());
    assert!(w.error.is_some(), "error banner should be set");
}

#[tokio::test]
async fn atomic_save_preserves_tts_and_generation_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role":"assistant","content":"pong"}}]
        })))
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);

    // Pre-write a config.toml with TTS fields populated.
    let mut existing = Config::default();
    existing.tts.iflytek = IflytekConfig {
        app_id: "APP123".into(),
        api_key: "KEY456".into(),
        api_secret: "SEC789".into(),
        voice: "x3_xiaoyan".into(),
    };
    existing.generation.max_concurrent_calls = 7;
    existing.write_atomic(&paths.config_file).unwrap();

    let (mut app, _rx) = make_app(paths.clone(), progress, existing);

    app.open_wizard(WizardOrigin::Command);
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.draft.llm.base_url = server.uri();
        w.draft.llm.api_key = "sk-new".into();
        w.draft.llm.model = "new-model".into();
        w.step = WizardStep::Model;
    }
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::ConnectivityOk));

    // Now at TtsEnable step with input pre-seeded to "y" (default enabled).
    // Clear and type "n" to decline TTS, then press Enter.
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)));
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)));
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    let saved = Config::load(&paths.config_file).unwrap();
    assert_eq!(saved.llm.base_url, server.uri());
    assert_eq!(saved.tts.iflytek.app_id, "APP123");
    assert_eq!(saved.tts.iflytek.voice, "x3_xiaoyan");
    assert_eq!(saved.generation.max_concurrent_calls, 7);
}

#[tokio::test]
async fn config_command_opens_wizard_with_command_origin() {
    // REGRESSION guard for 4b88dd1: palette Enter must not override screen set by execute_command.
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let mut cfg = Config::default();
    cfg.llm.api_key = "sk-existing".into();
    let (mut app, _rx) = make_app(paths, progress, cfg);

    // Drive the full palette path: Ctrl+P → type "config" → Enter.
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    app.on_input(Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )));
    for c in "config".chars() {
        app.on_input(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    app.on_input(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));

    assert!(matches!(app.screen, Screen::ConfigWizard));
    let w = app.config_wizard.as_ref().unwrap();
    assert_eq!(w.origin, WizardOrigin::Command);
    // api_key was pre-seeded into draft from the active config.
    assert_eq!(w.draft.llm.api_key, "sk-existing");
}

#[tokio::test]
async fn tts_probe_success_saves_and_dismisses() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths.clone(), progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.draft.llm.base_url = "https://x/v1".into();
        w.draft.llm.api_key = "sk-test".into();
        w.draft.llm.model = "gpt-4o-mini".into();
        w.draft.tts.iflytek.app_id = "app123".into();
        w.draft.tts.iflytek.api_key = "key456".into();
        w.draft.tts.iflytek.api_secret = "sec789".into();
        w.tts_enabled = true;
        w.step = WizardStep::TtsApiSecret;
    }

    // Simulate TTS probe success
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::TtsProbeOk));

    assert!(matches!(app.screen, Screen::Study));
    assert!(app.config_wizard.is_none());
    let saved = Config::load(&paths.config_file).unwrap();
    assert_eq!(saved.tts.iflytek.app_id, "app123");
    assert!(saved.tts.enabled);
}

#[tokio::test]
async fn tts_probe_failed_keeps_wizard_open() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let (mut app, _rx) = make_app(paths, progress, Config::default());

    app.open_wizard(WizardOrigin::FirstRun);
    {
        let w = app.config_wizard.as_mut().unwrap();
        w.tts_enabled = true;
        w.step = WizardStep::TtsApiSecret;
    }

    let err = inkworm::error::AppError::Tts(inkworm::tts::speaker::TtsError::Auth("bad creds".into()));
    app.on_task_msg(TaskMsg::Wizard(WizardTaskMsg::TtsProbeFailed(err)));

    assert!(matches!(app.screen, Screen::ConfigWizard));
    let w = app.config_wizard.as_ref().unwrap();
    assert!(w.testing.is_none());
    assert!(w.error.is_some());
}

#[tokio::test]
async fn esc_on_endpoint_command_origin_aborts_wizard() {
    let tmp = TempDir::new().unwrap();
    let (paths, progress) = setup(&tmp);
    let mut cfg = Config::default();
    cfg.llm.api_key = "sk-existing".into();
    let (mut app, _rx) = make_app(paths, progress, cfg);

    app.open_wizard(WizardOrigin::Command);
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    app.on_input(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));

    assert!(matches!(app.screen, Screen::Study));
    assert!(app.config_wizard.is_none());
}
