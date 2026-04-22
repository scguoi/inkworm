//! Integration test: IflytekSpeaker against a local tokio-tungstenite mock.

use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures::{SinkExt, StreamExt};
use inkworm::config::IflytekConfig;
use inkworm::tts::speaker::Speaker;
use inkworm::tts::IflytekSpeaker;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn cfg() -> IflytekConfig {
    IflytekConfig {
        app_id: "app".into(),
        api_key: "key".into(),
        api_secret: "secret".into(),
        voice: "x3_catherine".into(),
    }
}

/// Spin up a one-shot mock WS server that replies with three response frames
/// (status 0 + 1 + 2) carrying `total_samples` i16 PCM samples split across
/// them. Returns the `ws://127.0.0.1:<port>/` URL and the server join handle.
async fn start_mock_server(total_samples: u32) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}/");

    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();

        // Read the single request frame (body ignored).
        let _req = ws.next().await.unwrap().unwrap();

        // Build three chunks of equal-ish length.
        let mut samples: Vec<i16> = (0..total_samples as usize)
            .map(|i| (i as i16).wrapping_mul(7))
            .collect();
        let third = samples.len() / 3;
        let chunk_c: Vec<i16> = samples.split_off(2 * third);
        let chunk_b: Vec<i16> = samples.split_off(third);
        let chunk_a: Vec<i16> = samples;

        for (i, chunk) in [chunk_a, chunk_b, chunk_c].iter().enumerate() {
            let status = match i {
                0 => 0,
                1 => 1,
                _ => 2,
            };
            let bytes: Vec<u8> = chunk.iter().flat_map(|s| s.to_le_bytes()).collect();
            let audio_b64 = BASE64.encode(&bytes);
            let frame = format!(
                r#"{{"code":0,"message":"success","sid":"test","data":{{"status":{status},"audio":"{audio_b64}","ced":"{i}"}}}}"#,
            );
            ws.send(Message::Text(frame)).await.unwrap();
        }
        let _ = ws.send(Message::Close(None)).await;
    });
    (url, handle)
}

#[tokio::test]
async fn speaker_streams_frames_and_writes_cache() {
    let (url, server) = start_mock_server(240).await;
    let tmp = tempfile::tempdir().unwrap();
    let speaker = IflytekSpeaker::with_base_url(cfg(), tmp.path().to_path_buf(), None, url);

    let res = speaker.speak("hello world").await;
    assert!(res.is_ok(), "{res:?}");
    let _ = server.await;

    let key = inkworm::tts::cache::cache_key("hello world", "x3_catherine");
    let path = inkworm::tts::cache::cache_path(tmp.path(), &key);
    assert!(path.exists(), "cache file should be written");

    let got = inkworm::tts::wav::read_wav_pcm(&path).unwrap();
    assert_eq!(got.len(), 240);
}

#[tokio::test]
async fn second_speak_with_same_text_is_cache_hit() {
    let (url, server) = start_mock_server(120).await;
    let tmp = tempfile::tempdir().unwrap();
    let speaker = Arc::new(IflytekSpeaker::with_base_url(
        cfg(),
        tmp.path().to_path_buf(),
        None,
        url,
    ));
    speaker.speak("same text").await.unwrap();
    let _ = server.await;

    // Second call MUST be cache hit: no second server exists, so if it tried
    // WS it would fail.
    speaker.speak("same text").await.unwrap();
}

#[tokio::test]
async fn cancel_during_stream_returns_cancelled_error() {
    // Custom server that sends one frame then stalls indefinitely.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}/");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let _req = ws.next().await.unwrap().unwrap();
        let frame =
            r#"{"code":0,"message":"success","sid":"x","data":{"status":0,"audio":"","ced":"0"}}"#;
        ws.send(Message::Text(frame.into())).await.unwrap();
        tokio::time::sleep(Duration::from_secs(60)).await;
    });

    let tmp = tempfile::tempdir().unwrap();
    let speaker = Arc::new(IflytekSpeaker::with_base_url(
        cfg(),
        tmp.path().to_path_buf(),
        None,
        url,
    ));
    let speaker_bg = Arc::clone(&speaker);
    let task = tokio::spawn(async move { speaker_bg.speak("cancel me").await });

    // Give the speaker time to connect and receive the first frame.
    tokio::time::sleep(Duration::from_millis(150)).await;
    speaker.cancel();

    let res = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("task should finish within 2s after cancel");
    let res = res.expect("task panicked");
    assert!(
        matches!(res, Err(inkworm::tts::speaker::TtsError::Cancelled)),
        "{res:?}"
    );

    let key = inkworm::tts::cache::cache_key("cancel me", "x3_catherine");
    let path = inkworm::tts::cache::cache_path(tmp.path(), &key);
    assert!(!path.exists());
}
