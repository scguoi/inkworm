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
