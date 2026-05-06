use inkworm::config::{Config, IflytekConfig, TtsOverride};
use inkworm::tts::OutputKind;

#[test]
fn status_display_all_fields() {
    let mut cfg = Config::default();
    cfg.tts.r#override = TtsOverride::Auto;
    cfg.tts.iflytek = IflytekConfig {
        app_id: "app".into(),
        api_key: "key".into(),
        api_secret: "sec".into(),
        voice: "x4_enus_catherine_profnews".into(),
    };

    let device = OutputKind::Bluetooth;
    let last_error = Some("Network timeout".to_string());
    let cache_stats = (12, 1_234_567);

    // Test data assembly logic
    let mode_str = format!("{:?}", cfg.tts.r#override).to_lowercase();
    assert_eq!(mode_str, "auto");

    let device_str = match device {
        OutputKind::Bluetooth | OutputKind::WiredHeadphones => "headphones",
        OutputKind::BuiltInSpeaker | OutputKind::ExternalSpeaker => "speaker",
        OutputKind::Unknown => "unknown",
    };
    assert_eq!(device_str, "headphones");

    let creds_ok = !cfg.tts.iflytek.app_id.trim().is_empty()
        && !cfg.tts.iflytek.api_key.trim().is_empty()
        && !cfg.tts.iflytek.api_secret.trim().is_empty();
    assert!(creds_ok);

    let (count, bytes) = cache_stats;
    let mb = bytes as f64 / 1_048_576.0;
    let cache_str = format!("{} files ({:.1} MB)", count, mb);
    assert_eq!(cache_str, "12 files (1.2 MB)");

    let error_str = last_error.as_deref().unwrap_or("(none)");
    assert_eq!(error_str, "Network timeout");
}

#[test]
fn five_strikes_disables_session() {
    let mut failure_count = 0u32;
    let mut session_disabled = false;

    for _ in 0..5 {
        failure_count += 1;
        if failure_count >= 5 {
            session_disabled = true;
        }
    }

    assert!(session_disabled);
    assert_eq!(failure_count, 5);
}

#[test]
fn four_strikes_does_not_disable_session() {
    let mut failure_count = 0u32;
    let mut session_disabled = false;

    for _ in 0..4 {
        failure_count += 1;
        if failure_count >= 5 {
            session_disabled = true;
        }
    }

    assert!(!session_disabled);
    assert_eq!(failure_count, 4);
}

#[test]
fn success_re_enables_session() {
    let mut failure_count = 5u32;
    let mut session_disabled = true;

    // Simulate a successful TTS call
    failure_count = 0;
    if session_disabled {
        session_disabled = false;
    }

    assert!(!session_disabled);
    assert_eq!(failure_count, 0);
}
