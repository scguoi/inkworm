mod common;

mod load_validation {
    use super::common::TestEnv;
    use inkworm::config::{Config, ConfigError, TtsOverride};

    #[test]
    fn default_validate_reports_missing_api_key() {
        let c = Config::default();
        let errs = c.validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::MissingField("llm.api_key"))),
            "{errs:#?}"
        );
    }

    #[test]
    fn fully_populated_validates_clean() {
        let mut c = Config::default();
        c.llm.api_key = "sk-test".into();
        c.tts.iflytek.app_id = "a".into();
        c.tts.iflytek.api_key = "b".into();
        c.tts.iflytek.api_secret = "c".into();
        assert!(c.validate().is_empty());
    }

    #[test]
    fn tts_disabled_skips_iflytek_validation() {
        let mut c = Config::default();
        c.llm.api_key = "sk".into();
        c.tts.enabled = false;
        assert!(c.validate().is_empty());
    }

    #[test]
    fn tts_override_off_skips_iflytek_validation() {
        let mut c = Config::default();
        c.llm.api_key = "sk".into();
        c.tts.r#override = TtsOverride::Off;
        assert!(c.validate().is_empty());
    }

    #[test]
    fn zero_concurrent_calls_invalid() {
        // Validate returns multiple errors here (iflytek creds are also
        // missing since tts defaults to enabled + auto); we only assert the
        // specific Invalid variant we're targeting.
        let mut c = Config::default();
        c.llm.api_key = "sk".into();
        c.generation.max_concurrent_calls = 0;
        let errs = c.validate();
        assert!(errs.iter().any(|e| matches!(
            e,
            ConfigError::Invalid {
                field: "generation.max_concurrent_calls",
                ..
            }
        )));
    }

    #[test]
    fn load_missing_file_returns_io_error() {
        let env = TestEnv::new();
        let path = env.home.join("nope.toml");
        let err = Config::load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Io(_)), "{err:?}");
    }

    #[test]
    fn toml_round_trips_through_disk() {
        let env = TestEnv::new();
        let path = env.home.join("config.toml");
        let mut c = Config::default();
        c.llm.api_key = "sk-1".into();
        c.tts.iflytek.app_id = "a".into();
        c.tts.iflytek.api_key = "b".into();
        c.tts.iflytek.api_secret = "c".into();
        c.write_atomic(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn unknown_fields_rejected() {
        let env = TestEnv::new();
        let path = env.home.join("config.toml");
        std::fs::write(&path, "schema_version = 1\nbogus = true\n").unwrap();
        assert!(matches!(
            Config::load(&path).unwrap_err(),
            ConfigError::Toml(_)
        ));
    }

    #[test]
    fn data_home_override_empty_string_returns_none() {
        let c = Config::default();
        assert_eq!(c.data_home_override(), None);
    }

    #[test]
    fn data_home_override_whitespace_returns_none() {
        let mut c = Config::default();
        c.data.home = "   ".into();
        assert_eq!(c.data_home_override(), None);
    }

    #[test]
    fn data_home_override_populated_returns_some() {
        use std::path::PathBuf;
        let mut c = Config::default();
        c.data.home = "/tmp/inkworm".into();
        assert_eq!(c.data_home_override(), Some(PathBuf::from("/tmp/inkworm")));
    }
}
