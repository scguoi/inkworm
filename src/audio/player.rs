//! Mp3 playback for course-bundled audio.
//!
//! `BundlePlayer` owns a single rodio `Sink` slot for the most recently
//! triggered bundle playback. Calling `play()` while audio is already
//! playing supersedes the previous sink (matches the IflytekSpeaker
//! convention). `cancel()` stops the active sink.

use std::path::Path;
use std::sync::{Arc, Mutex};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(String),
    #[error("audio: {0}")]
    Audio(String),
}

pub struct BundlePlayer {
    audio: Option<rodio::OutputStreamHandle>,
    current_sink: Arc<Mutex<Option<rodio::Sink>>>,
}

impl BundlePlayer {
    pub fn new(audio: Option<rodio::OutputStreamHandle>) -> Self {
        Self {
            audio,
            current_sink: Arc::new(Mutex::new(None)),
        }
    }

    /// Decode the mp3 at `path` and start playback. With `audio=None`
    /// (cache-only / headless mode used by tests) decode still runs so
    /// errors are observable, but no Sink is created.
    ///
    /// Replaces any prior sink. Decode runs on a blocking thread so the
    /// async caller is not stalled.
    pub async fn play(&self, path: &Path) -> Result<(), BundleError> {
        let path_owned = path.to_path_buf();
        let decoded: Result<rodio::Decoder<std::io::BufReader<std::fs::File>>, BundleError> =
            tokio::task::spawn_blocking(move || {
                let file = std::fs::File::open(&path_owned)?;
                let reader = std::io::BufReader::new(file);
                rodio::Decoder::new(reader).map_err(|e| BundleError::Decode(format!("{e}")))
            })
            .await
            .map_err(|e| BundleError::Audio(format!("join: {e}")))?;

        let source = decoded?;

        let Some(handle) = &self.audio else {
            // Cache-only mode: decode succeeded, drop the source.
            return Ok(());
        };
        let sink = rodio::Sink::try_new(handle).map_err(|e| BundleError::Audio(e.to_string()))?;
        sink.append(source);
        if let Ok(mut guard) = self.current_sink.lock() {
            if let Some(old) = guard.take() {
                old.stop();
            }
            *guard = Some(sink);
        }
        Ok(())
    }

    /// Stop any currently-playing sink. Safe when nothing is playing.
    pub fn cancel(&self) {
        if let Ok(mut guard) = self.current_sink.lock() {
            if let Some(sink) = guard.take() {
                sink.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn play_with_no_audio_handle_decodes_real_mp3_fixture() {
        let player = BundlePlayer::new(None);
        let res = player.play(Path::new("fixtures/audio/silence.mp3")).await;
        assert!(res.is_ok(), "expected real fixture to decode, got {res:?}");
    }

    #[tokio::test]
    async fn play_missing_file_returns_io_error() {
        let player = BundlePlayer::new(None);
        let err = player
            .play(Path::new("/definitely/does/not/exist.mp3"))
            .await
            .unwrap_err();
        assert!(matches!(err, BundleError::Io(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn play_corrupt_file_returns_decode_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.mp3");
        // Random non-mp3 bytes; minimp3 should reject.
        std::fs::write(&path, b"not an mp3 at all, just text").unwrap();
        let player = BundlePlayer::new(None);
        let err = player.play(&path).await.unwrap_err();
        assert!(
            matches!(err, BundleError::Decode(_)),
            "expected Decode, got {err:?}"
        );
    }

    #[tokio::test]
    async fn play_zero_byte_file_returns_decode_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.mp3");
        std::fs::write(&path, b"").unwrap();
        let player = BundlePlayer::new(None);
        let err = player.play(&path).await.unwrap_err();
        assert!(
            matches!(err, BundleError::Decode(_)),
            "expected Decode, got {err:?}"
        );
    }

    #[test]
    fn cancel_without_active_play_is_noop() {
        let player = BundlePlayer::new(None);
        player.cancel(); // must not panic
    }
}
