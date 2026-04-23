mod common;

mod client {
    use super::common::llm_mocks::{envelope, expect_ok, expect_status};
    use inkworm::llm::client::{LlmClient, ReqwestClient};
    use inkworm::llm::error::LlmError;
    use inkworm::llm::types::ChatRequest;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_client(base_url: String) -> ReqwestClient {
        ReqwestClient::new(base_url, "sk-test", Duration::from_secs(5)).unwrap()
    }

    fn req() -> ChatRequest {
        ChatRequest::system_and_user("m", "sys".into(), "usr".into())
    }

    #[tokio::test]
    async fn ok_returns_content() {
        let server = MockServer::start().await;
        expect_ok(&server, "hello").await;
        let c = make_client(server.uri());
        let r = c.chat(req(), CancellationToken::new()).await.unwrap();
        assert_eq!(r, "hello");
    }

    #[tokio::test]
    async fn unauthorized_maps_to_unauthorized() {
        let server = MockServer::start().await;
        expect_status(&server, 401, "nope").await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, LlmError::Unauthorized), "{err:?}");
    }

    #[tokio::test]
    async fn forbidden_maps_to_unauthorized() {
        let server = MockServer::start().await;
        expect_status(&server, 403, "nope").await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, LlmError::Unauthorized), "{err:?}");
    }

    #[tokio::test]
    async fn server_error_captures_status_and_body() {
        let server = MockServer::start().await;
        expect_status(&server, 503, "overloaded").await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        match err {
            LlmError::Server { status, body } => {
                assert_eq!(status, 503);
                assert!(body.contains("overloaded"));
            }
            other => panic!("expected Server, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rate_limit_extracts_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "7")
                    .set_body_string("slow down"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        match err {
            LlmError::RateLimited(Some(d)) => assert_eq!(d, Duration::from_secs(7)),
            other => panic!("expected RateLimited with retry-after, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancellation_short_circuits() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(envelope("late"))
                    .set_delay(Duration::from_secs(5)),
            )
            .expect(1)
            .mount(&server)
            .await;
        let c = make_client(server.uri());
        let token = CancellationToken::new();
        let t2 = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            t2.cancel();
        });
        let err = c.chat(req(), token).await.unwrap_err();
        assert!(matches!(err, LlmError::Cancelled), "{err:?}");
    }

    #[tokio::test]
    async fn malformed_body_maps_to_invalid_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .expect(1)
            .mount(&server)
            .await;
        let c = make_client(server.uri());
        let err = c.chat(req(), CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, LlmError::InvalidResponse(_)), "{err:?}");
    }
}

mod prompts {
    use inkworm::llm::prompt::{phase1_system, PHASE2_SYSTEM, REPAIR_TEMPLATE};

    #[test]
    fn phase1_system_snapshot() {
        let intermediate_desc = "intermediate (CEFR B1-B2): select sentences with moderate complexity. Skip very simple sentences and extremely advanced ones. Focus on useful grammar patterns and practical vocabulary.";
        insta::assert_snapshot!("phase1_system", phase1_system(intermediate_desc));
    }

    #[test]
    fn phase2_system_snapshot() {
        insta::assert_snapshot!("phase2_system", PHASE2_SYSTEM);
    }

    #[test]
    fn repair_template_snapshot() {
        insta::assert_snapshot!("repair_template", REPAIR_TEMPLATE);
    }
}

mod reflexion_phase1 {
    use super::common::llm_mocks::envelope;
    use super::common::TestEnv;
    use chrono::{TimeZone, Utc};
    use inkworm::clock::FixedClock;
    use inkworm::llm::client::ReqwestClient;
    use inkworm::llm::reflexion::{Reflexion, ReflexionError};
    use inkworm::storage::paths::DataPaths;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make(env: &TestEnv, server: &MockServer) -> (DataPaths, FixedClock, ReqwestClient) {
        let paths = DataPaths::resolve(Some(&env.home)).unwrap();
        paths.ensure_dirs().unwrap();
        let clock = FixedClock(Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap());
        let client = ReqwestClient::new(server.uri(), "sk-test", Duration::from_secs(5)).unwrap();
        (paths, clock, client)
    }

    fn ok_phase1() -> &'static str {
        r#"{
          "title": "AI at work",
          "description": "",
          "sentences": [
            {"chinese": "一", "english": "one two three"},
            {"chinese": "二", "english": "four five six"},
            {"chinese": "三", "english": "seven eight nine"},
            {"chinese": "四", "english": "ten eleven twelve"},
            {"chinese": "五", "english": "thirteen fourteen fifteen"}
          ]
        }"#
    }

    #[tokio::test]
    async fn phase1_happy_path() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope(ok_phase1())))
            .expect(1)
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "gpt-4o-mini",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let out = r.reflexion_split("article text", "intermediate").await.unwrap();
        assert_eq!(out.sentences.len(), 5);
        assert_eq!(out.title, "AI at work");
        // No failed/ report written.
        let failed_dir_entries: Vec<_> = std::fs::read_dir(&paths.failed_dir).unwrap().collect();
        assert!(failed_dir_entries.is_empty());
    }

    #[tokio::test]
    async fn phase1_repair_success_on_second_attempt() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        // First response: missing sentences.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(envelope(r#"{"title":"T","description":""}"#)),
            )
            .up_to_n_times(1)
            .expect(1)
            .named("bad-first")
            .mount(&server)
            .await;
        // Second response: valid.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope(ok_phase1())))
            .expect(1)
            .named("good-second")
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let out = r.reflexion_split("article", "intermediate").await.unwrap();
        assert_eq!(out.sentences.len(), 5);
    }

    #[tokio::test]
    async fn phase1_three_failures_saves_to_disk_and_errors() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope("not json at all")))
            .expect(3)
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let err = r.reflexion_split("article", "intermediate").await.unwrap_err();
        match err {
            ReflexionError::AllAttemptsFailed {
                phase,
                saved_to,
                last_attempts,
                ..
            } => {
                assert_eq!(phase, 1);
                assert!(saved_to.exists());
                assert_eq!(last_attempts.len(), 3);
            }
            other => panic!("expected AllAttemptsFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn phase1_auth_error_short_circuits_no_retry() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("nope"))
            .expect(1) // only ONE call; auth error does not retry
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let err = r.reflexion_split("article", "intermediate").await.unwrap_err();
        assert!(matches!(err, ReflexionError::Llm(_)), "{err:?}");
        // No failed/ report for transport errors.
        let failed: Vec<_> = std::fs::read_dir(&paths.failed_dir).unwrap().collect();
        assert!(failed.is_empty());
    }

    #[tokio::test]
    async fn phase1_cancel_stops_retry_loop() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        // Bad JSON response with a small delay so we have time to cancel between retries.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(envelope("not json"))
                    .set_delay(Duration::from_millis(50)),
            )
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            c2.cancel();
        });
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel,
        };
        let err = r.reflexion_split("article", "intermediate").await.unwrap_err();
        assert!(
            matches!(
                err,
                ReflexionError::Cancelled
                    | ReflexionError::Llm(inkworm::llm::error::LlmError::Cancelled)
            ),
            "{err:?}"
        );
    }
}

mod reflexion_phase2 {
    use super::common::llm_mocks::envelope;
    use super::common::TestEnv;
    use chrono::{TimeZone, Utc};
    use inkworm::clock::FixedClock;
    use inkworm::llm::client::ReqwestClient;
    use inkworm::llm::reflexion::{Reflexion, ReflexionError};
    use inkworm::llm::types::RawSentence;
    use inkworm::storage::paths::DataPaths;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ok_drills(english: &str) -> String {
        format!(
            r#"{{
                "drills": [
                    {{"stage": 1, "focus": "keywords", "chinese": "关键", "english": "a b", "soundmark": ""}},
                    {{"stage": 2, "focus": "skeleton", "chinese": "骨架", "english": "a b c", "soundmark": ""}},
                    {{"stage": 3, "focus": "full", "chinese": "完整", "english": "{english}", "soundmark": ""}}
                ]
            }}"#
        )
    }

    fn five_sentences() -> Vec<RawSentence> {
        (0..5)
            .map(|i| RawSentence {
                chinese: format!("句{i}"),
                english: format!("sentence number {i} here"),
            })
            .collect()
    }

    fn make(env: &TestEnv, server: &MockServer) -> (DataPaths, FixedClock, ReqwestClient) {
        let paths = DataPaths::resolve(Some(&env.home)).unwrap();
        paths.ensure_dirs().unwrap();
        let clock = FixedClock(Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap());
        let client = ReqwestClient::new(server.uri(), "sk-test", Duration::from_secs(5)).unwrap();
        (paths, clock, client)
    }

    #[tokio::test]
    async fn phase2_all_ok() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        let sentences = five_sentences();
        for s in &sentences {
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains(&s.english as &str))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(envelope(&ok_drills(&s.english))),
                )
                .up_to_n_times(1)
                .expect(1)
                .named(format!("drill-{}", s.english))
                .mount(&server)
                .await;
        }
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 3,
            cancel: CancellationToken::new(),
        };
        let outs = r.orchestrate_phase2(&sentences, None).await.unwrap();
        assert_eq!(outs.len(), 5);
        for o in &outs {
            assert_eq!(o.drills.len(), 3);
        }
    }

    #[tokio::test]
    async fn phase2_one_sentence_failure_fails_the_whole_run() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        let sentences = five_sentences();
        // 4 sentences respond fine (match by their unique english substring).
        for s in sentences.iter().take(4) {
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains(&s.english as &str))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(envelope(&ok_drills(&s.english))),
                )
                .up_to_n_times(1)
                .expect(1)
                .named(format!("ok-{}", s.english))
                .mount(&server)
                .await;
        }
        // The 5th sentence's prompt (which contains "sentence number 4 here") fails 3x.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("sentence number 4 here"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope("not json")))
            .expect(3)
            .named("bad-sentence-4")
            .mount(&server)
            .await;
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let err = r.orchestrate_phase2(&sentences, None).await.unwrap_err();
        match err {
            ReflexionError::AllAttemptsFailed {
                phase,
                sentence_index,
                ..
            } => {
                assert_eq!(phase, 2);
                assert_eq!(sentence_index, Some(4));
            }
            other => panic!("expected AllAttemptsFailed phase=2, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn phase2_rejects_last_english_mismatch() {
        let env = TestEnv::new();
        let server = MockServer::start().await;
        let sentences = five_sentences();
        // For sentence 0, the drill returns english "WRONG" on the last drill,
        // causing RawDrills::validate to flag the mismatch — and after 3
        // failures the whole run errors.
        let bad = r#"{
          "drills": [
            {"stage": 1, "focus": "keywords", "chinese": "关键", "english": "a b", "soundmark": ""},
            {"stage": 2, "focus": "skeleton", "chinese": "骨架", "english": "a b c", "soundmark": ""},
            {"stage": 3, "focus": "full", "chinese": "完整", "english": "WRONG", "soundmark": ""}
          ]
        }"#;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("sentence number 0 here"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope(bad)))
            .expect(3)
            .named("mismatch")
            .mount(&server)
            .await;
        // The other 4 succeed on first try.
        for s in sentences.iter().skip(1) {
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains(&s.english as &str))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(envelope(&ok_drills(&s.english))),
                )
                .up_to_n_times(1)
                .expect(1)
                .named(format!("ok-{}", s.english))
                .mount(&server)
                .await;
        }
        let (paths, clock, client) = make(&env, &server);
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "m",
            max_concurrent: 5,
            cancel: CancellationToken::new(),
        };
        let err = r.orchestrate_phase2(&sentences, None).await.unwrap_err();
        assert!(
            matches!(
                err,
                ReflexionError::AllAttemptsFailed {
                    phase: 2,
                    sentence_index: Some(0),
                    ..
                }
            ),
            "{err:?}"
        );
    }
}

mod reflexion_e2e {
    use super::common::llm_mocks::envelope;
    use super::common::TestEnv;
    use chrono::{TimeZone, Utc};
    use inkworm::clock::FixedClock;
    use inkworm::llm::client::ReqwestClient;
    use inkworm::llm::reflexion::Reflexion;
    use inkworm::storage::paths::DataPaths;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn phase1_ok() -> &'static str {
        r#"{
          "title": "Test lesson",
          "description": "",
          "sentences": [
            {"chinese": "一", "english": "sentence number 0 here"},
            {"chinese": "二", "english": "sentence number 1 here"},
            {"chinese": "三", "english": "sentence number 2 here"},
            {"chinese": "四", "english": "sentence number 3 here"},
            {"chinese": "五", "english": "sentence number 4 here"}
          ]
        }"#
    }

    fn drill_for(english: &str) -> String {
        format!(
            r#"{{
              "drills": [
                {{"stage": 1, "focus": "keywords", "chinese": "关键", "english": "a b", "soundmark": "/eɪ/ /biː/"}},
                {{"stage": 2, "focus": "skeleton", "chinese": "骨架", "english": "a b c", "soundmark": "/eɪ/ /biː/ /siː/"}},
                {{"stage": 3, "focus": "full", "chinese": "完整", "english": "{english}", "soundmark": "/eɪ/ /biː/ /siː/"}}
              ]
            }}"#
        )
    }

    #[tokio::test]
    async fn article_to_course_happy_path() {
        let env = TestEnv::new();
        let server = MockServer::start().await;

        // Phase 1 — matches the article text in the user prompt.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("Article to split"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope(phase1_ok())))
            .up_to_n_times(1)
            .expect(1)
            .named("phase1")
            .mount(&server)
            .await;

        // Phase 2 — one mock per sentence, keyed by matching the english in the
        // user prompt (each request body embeds the sentence JSON).
        for i in 0..5 {
            let english = format!("sentence number {i} here");
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_string_contains(&english as &str))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(envelope(&drill_for(&english))),
                )
                .up_to_n_times(1)
                .expect(1)
                .named(format!("phase2-{i}"))
                .mount(&server)
                .await;
        }

        let paths = DataPaths::resolve(Some(&env.home)).unwrap();
        paths.ensure_dirs().unwrap();
        let clock = FixedClock(Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap());
        let client = ReqwestClient::new(server.uri(), "sk-test", Duration::from_secs(5)).unwrap();
        let r = Reflexion {
            client: &client,
            clock: &clock,
            paths: &paths,
            model: "gpt-4o-mini",
            max_concurrent: 3,
            cancel: CancellationToken::new(),
        };

        let out = r
            .generate("This is a sample article body with enough context.", "intermediate", &[], None)
            .await
            .unwrap();

        // Course-level invariants.
        assert_eq!(out.course.schema_version, 2);
        assert!(out.course.id.starts_with("2026-04-21-test-lesson"));
        assert_eq!(out.course.title, "Test lesson");
        assert_eq!(out.course.sentences.len(), 5);
        assert!(out.course.sentences.iter().all(|s| s.drills.len() == 3));
        assert!(
            out.course.validate().is_empty(),
            "{:#?}",
            out.course.validate()
        );
    }
}
