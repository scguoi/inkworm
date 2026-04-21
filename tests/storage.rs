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

    #[test]
    fn good_maximal_validates() {
        let json = load("good/maximal.json");
        let course: Course = serde_json::from_str(&json).expect("deserialize");
        let errs = course.validate();
        assert!(errs.is_empty(), "unexpected errors: {errs:#?}");
        assert_eq!(course.sentences.len(), 20);
        assert!(course.sentences.iter().all(|s| s.drills.len() == 5));
    }

    #[test]
    fn good_soundmark_empty_validates() {
        let json = load("good/soundmark_empty.json");
        let course: Course = serde_json::from_str(&json).expect("deserialize");
        let errs = course.validate();
        assert!(errs.is_empty(), "unexpected errors: {errs:#?}");
        assert!(course
            .sentences
            .iter()
            .all(|s| s.drills.iter().all(|d| d.soundmark.is_empty())));
    }
}

mod course_bad {
    use inkworm::storage::course::{Course, ValidationError};

    fn load(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/courses")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
    }

    fn parse(name: &str) -> Course {
        let json = load(name);
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("parse {name}: {e}"))
    }

    #[test]
    fn wrong_schema_version_reported() {
        let errs = parse("bad/schema_version_wrong.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::WrongSchemaVersion { .. })),
            "{errs:#?}"
        );
    }

    #[test]
    fn sentences_too_few_reported() {
        let errs = parse("bad/sentences_too_few.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::SentencesCount(4))),
            "{errs:#?}"
        );
    }

    #[test]
    fn sentences_too_many_reported() {
        let errs = parse("bad/sentences_too_many.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::SentencesCount(21))),
            "{errs:#?}"
        );
    }

    #[test]
    fn drills_too_few_reported() {
        let errs = parse("bad/drills_too_few.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::DrillsCount { count: 2, .. })),
            "{errs:#?}"
        );
    }

    #[test]
    fn drills_too_many_reported() {
        let errs = parse("bad/drills_too_many.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::DrillsCount { count: 6, .. })),
            "{errs:#?}"
        );
    }

    #[test]
    fn last_drill_not_full_reported() {
        let errs = parse("bad/last_drill_not_full.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::LastDrillNotFull { .. })),
            "{errs:#?}"
        );
    }

    #[test]
    fn stage_not_monotonic_reported() {
        let errs = parse("bad/stage_not_monotonic.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::DrillStage { .. })),
            "{errs:#?}"
        );
    }

    #[test]
    fn order_not_monotonic_reported() {
        let errs = parse("bad/order_not_monotonic.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::SentenceOrder { .. })),
            "{errs:#?}"
        );
    }

    #[test]
    fn chinese_too_long_reported() {
        let errs = parse("bad/chinese_too_long.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::ChineseLength { .. })),
            "{errs:#?}"
        );
    }

    #[test]
    fn invalid_focus_fails_to_deserialize() {
        let json = load("bad/invalid_focus.json");
        let r: Result<Course, _> = serde_json::from_str(&json);
        assert!(r.is_err(), "expected deserialize failure");
    }

    #[test]
    fn invalid_soundmark_reported() {
        let errs = parse("bad/invalid_soundmark.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::SoundmarkFormat { .. })),
            "{errs:#?}"
        );
    }

    #[test]
    fn empty_title_reported() {
        let errs = parse("bad/empty_title.json").validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::TitleLength(0))),
            "{errs:#?}"
        );
    }

    #[test]
    fn validation_returns_all_errors_not_just_first() {
        // Construct a course with multiple simultaneous violations and assert
        // that validate() reports all of them, not just the first encountered.
        use chrono::Utc;
        use inkworm::storage::course::{Source, SourceKind};

        let c = Course {
            schema_version: 2,
            id: "multi-error-test".into(),
            title: String::new(), // → TitleLength(0)
            description: None,
            source: Source {
                kind: SourceKind::Article,
                url: String::new(),
                created_at: Utc::now(),
                model: "test".into(),
            },
            sentences: vec![], // → SentencesCount(0)
        };
        let errs = c.validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::TitleLength(0))),
            "missing TitleLength; got {errs:#?}"
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::SentencesCount(0))),
            "missing SentencesCount; got {errs:#?}"
        );
        assert!(
            errs.len() >= 2,
            "expected ≥2 errors, got {}: {errs:#?}",
            errs.len()
        );
    }
}

mod course_crud {
    use super::common::TestEnv;
    use inkworm::storage::course::{
        delete_course, list_courses, load_course, save_course, Course, StorageError,
    };

    fn fixture_minimal() -> Course {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/courses/good/minimal.json");
        let json = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn save_then_load_round_trips() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        let loaded = load_course(&dir, &c.id).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn list_courses_returns_all_saved() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        c.id = "2026-04-21-second".into();
        save_course(&dir, &c).unwrap();
        let mut metas = list_courses(&dir).unwrap();
        metas.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].id, "2026-04-21-second");
    }

    #[test]
    fn list_empty_dir_returns_empty() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let metas = list_courses(&dir).unwrap();
        assert!(metas.is_empty());
    }

    #[test]
    fn list_nonexistent_dir_returns_empty() {
        let env = TestEnv::new();
        let dir = env.home.join("no-such");
        let metas = list_courses(&dir).unwrap();
        assert!(metas.is_empty());
    }

    #[test]
    fn load_missing_returns_not_found() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let err = load_course(&dir, "does-not-exist").unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[test]
    fn delete_removes_file() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        delete_course(&dir, &c.id).unwrap();
        assert!(matches!(
            load_course(&dir, &c.id).unwrap_err(),
            StorageError::NotFound(_)
        ));
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(
            delete_course(&dir, "no").unwrap_err(),
            StorageError::NotFound(_)
        ));
    }

    #[test]
    fn save_overwrites_existing() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        c.title = "Updated title".into();
        save_course(&dir, &c).unwrap();
        let loaded = load_course(&dir, &c.id).unwrap();
        assert_eq!(loaded.title, "Updated title");
    }

    #[test]
    fn list_skips_corrupt_json_file() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        // Drop a malformed file beside the good one.
        std::fs::write(dir.join("broken.json"), b"{ not valid json").unwrap();
        let metas = list_courses(&dir).unwrap();
        // Only the valid course appears.
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].id, c.id);
    }

    #[test]
    fn list_skips_unreadable_non_json() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        // A README-style file that isn't .json should be filtered by extension,
        // not skipped silently as a corrupt-json case.
        std::fs::write(dir.join("README.md"), b"not a course").unwrap();
        let metas = list_courses(&dir).unwrap();
        assert_eq!(metas.len(), 1);
    }
}

mod course_list_meta {
    use inkworm::storage::course::list_courses;

    #[test]
    fn list_courses_populates_total_drills() {
        let dir = tempfile::tempdir().unwrap();
        let json = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/courses/good/minimal.json"),
        )
        .unwrap();
        std::fs::write(dir.path().join("a.json"), &json).unwrap();

        let metas = list_courses(dir.path()).unwrap();
        assert_eq!(metas.len(), 1);
        // minimal.json fixture: 5 sentences × 3 drills each = 15.
        assert_eq!(metas[0].total_drills, 15);
    }

    #[test]
    fn list_courses_sorted_newest_first() {
        use chrono::{TimeZone, Utc};

        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/courses/good/minimal.json"),
        )
        .unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&base).unwrap();
        for (fname, date, id) in [
            ("old.json", "2026-01-01T00:00:00Z", "old"),
            ("newest.json", "2026-04-15T00:00:00Z", "newest"),
            ("mid.json", "2026-03-01T00:00:00Z", "mid"),
        ] {
            v["id"] = serde_json::Value::String(id.into());
            v["source"]["createdAt"] = serde_json::Value::String(date.into());
            std::fs::write(dir.path().join(fname), serde_json::to_vec(&v).unwrap()).unwrap();
        }

        let metas = list_courses(dir.path()).unwrap();
        assert_eq!(
            metas.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["newest", "mid", "old"]
        );
        assert_eq!(
            metas[0].created_at,
            Utc.with_ymd_and_hms(2026, 4, 15, 0, 0, 0).unwrap()
        );
    }
}

mod progress {
    use super::common::TestEnv;
    use chrono::{TimeZone, Utc};
    use inkworm::storage::course::Course;
    use inkworm::storage::progress::{course_stats, CourseProgress, DrillProgress, Progress};

    fn fixture_minimal() -> Course {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/courses/good/minimal.json");
        let json = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn load_missing_returns_empty() {
        let env = TestEnv::new();
        let p = Progress::load(&env.home.join("progress.json")).unwrap();
        assert_eq!(p.courses.len(), 0);
        assert_eq!(p.schema_version, 1);
    }

    #[test]
    fn save_then_load_round_trips() {
        let env = TestEnv::new();
        let path = env.home.join("progress.json");

        let mut p = Progress::empty();
        p.active_course_id = Some("c1".into());
        let cp = p.course_mut("c1");
        cp.last_studied_at = Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap();
        let sp = cp.sentences.entry("1".into()).or_default();
        sp.drills.insert(
            "1".into(),
            DrillProgress {
                mastered_count: 3,
                last_correct_at: Some(Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap()),
            },
        );
        p.save(&path).unwrap();

        let loaded = Progress::load(&path).unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn course_stats_total_matches_drills_sum() {
        let c = fixture_minimal();
        let stats = course_stats(&c, None);
        let expected: usize = c.sentences.iter().map(|s| s.drills.len()).sum();
        assert_eq!(stats.total_drills, expected);
        assert_eq!(stats.completed_drills, 0);
        assert_eq!(stats.percent(), 0);
    }

    #[test]
    fn course_stats_counts_mastered_drills() {
        let c = fixture_minimal();
        let mut cp = CourseProgress::default();
        let sp = cp.sentences.entry("1".into()).or_default();
        sp.drills.insert(
            "1".into(),
            DrillProgress {
                mastered_count: 2,
                last_correct_at: None,
            },
        );
        sp.drills.insert(
            "2".into(),
            DrillProgress {
                mastered_count: 0,
                last_correct_at: None,
            },
        );
        let stats = course_stats(&c, Some(&cp));
        assert_eq!(stats.completed_drills, 1);
    }
}
