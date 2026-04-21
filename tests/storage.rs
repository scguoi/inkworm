mod common;

mod paths {
    use super::common::TestEnv;
    use inkworm::storage::paths::DataPaths;
    use serial_test::serial;

    #[test]
    fn resolve_prefers_explicit_override() {
        let env = TestEnv::new();
        let paths = DataPaths::resolve(Some(&env.home)).expect("resolve");
        assert_eq!(paths.root, env.home);
    }

    #[test]
    #[serial]
    fn resolve_uses_inkworm_home_env_when_no_cli() {
        let env = TestEnv::new();
        std::env::set_var("INKWORM_HOME", &env.home);
        let paths = DataPaths::resolve(None).expect("resolve");
        assert_eq!(paths.root, env.home);
        std::env::remove_var("INKWORM_HOME");
    }

    #[test]
    #[serial]
    fn resolve_uses_xdg_config_home_when_set() {
        let env = TestEnv::new();
        let xdg_base = env.home.join("xdg");
        std::env::remove_var("INKWORM_HOME");
        std::env::set_var("XDG_CONFIG_HOME", &xdg_base);
        let paths = DataPaths::resolve(None).expect("resolve");
        assert_eq!(paths.root, xdg_base.join("inkworm"));
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    #[serial]
    fn resolve_falls_back_to_xdg_then_home() {
        std::env::remove_var("INKWORM_HOME");
        std::env::remove_var("XDG_CONFIG_HOME");
        // Returns a real path ending in "inkworm" from HOME fallback.
        let paths = DataPaths::resolve(None).expect("resolve");
        assert!(paths.root.ends_with("inkworm"));
    }

    #[test]
    fn ensure_dirs_creates_all_subdirs() {
        let env = TestEnv::new();
        let paths = DataPaths::resolve(Some(&env.home)).expect("resolve");
        paths.ensure_dirs().expect("ensure");
        assert!(paths.courses_dir.is_dir());
        assert!(paths.failed_dir.is_dir());
        assert!(paths.tts_cache_dir.is_dir());
    }

    #[test]
    fn derived_file_paths_match_root() {
        let env = TestEnv::new();
        let paths = DataPaths::resolve(Some(&env.home)).expect("resolve");
        assert_eq!(paths.config_file, env.home.join("config.toml"));
        assert_eq!(paths.progress_file, env.home.join("progress.json"));
        assert_eq!(paths.log_file, env.home.join("inkworm.log"));
    }
}

mod atomic_write {
    use super::common::TestEnv;
    use inkworm::storage::atomic::write_atomic;

    #[test]
    fn writes_full_content() {
        let env = TestEnv::new();
        let p = env.home.join("a.txt");
        write_atomic(&p, b"hello").expect("write");
        assert_eq!(std::fs::read(&p).unwrap(), b"hello");
    }

    #[test]
    fn overwrites_existing_file() {
        let env = TestEnv::new();
        let p = env.home.join("a.txt");
        std::fs::write(&p, b"old").unwrap();
        write_atomic(&p, b"new").expect("write");
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
    }

    #[test]
    fn creates_missing_parent_dir() {
        let env = TestEnv::new();
        let p = env.home.join("deep").join("nested").join("f.json");
        write_atomic(&p, b"{}").expect("write");
        assert!(p.is_file());
    }
}

mod course_schema {
    use inkworm::storage::course::Course;

    fn load(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/courses")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
    }

    #[test]
    fn good_minimal_round_trips() {
        let json = load("good/minimal.json");
        let course: Course = serde_json::from_str(&json).expect("deserialize");
        let errs = course.validate();
        assert!(errs.is_empty(), "unexpected errors: {errs:#?}");
        let reserialized = serde_json::to_string_pretty(&course).expect("serialize");
        let course2: Course = serde_json::from_str(&reserialized).expect("re-deserialize");
        assert_eq!(course, course2);
    }
}
