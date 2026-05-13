//! Course-bundled audio playback.
//!
//! `BundlePlayer` owns a single rodio `Sink` slot for the most recently
//! triggered bundle playback. Calling `play()` while audio is already
//! playing supersedes the previous sink (matches the IflytekSpeaker
//! convention). `cancel()` stops the active sink.
//!
//! Decoding is driven directly by `symphonia` rather than `rodio::Decoder`:
//! rodio 0.19's wrapper reports `byte_len() = None` and `unreachable!()`s
//! when the isomp4 demuxer tries an End-relative seek, which kills any
//! AAC-in-MP4 bundle (some TTS providers emit .m4a bytes under an `.mp3`
//! filename). Driving symphonia ourselves handles MP3 and AAC-in-MP4
//! uniformly.

use std::path::Path;
use std::sync::{Arc, Mutex};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
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

    /// Decode the audio file at `path` and start playback. With `audio=None`
    /// (cache-only / headless mode used by tests) decode still runs so
    /// errors are observable, but no Sink is created.
    ///
    /// Replaces any prior sink. Decode runs on a blocking thread so the
    /// async caller is not stalled.
    pub async fn play(&self, path: &Path) -> Result<(), BundleError> {
        let path_owned = path.to_path_buf();
        let decoded = tokio::task::spawn_blocking(move || decode_to_pcm(&path_owned))
            .await
            .map_err(|e| BundleError::Audio(format!("join: {e}")))?;

        let DecodedPcm {
            samples,
            sample_rate,
            channels,
        } = decoded?;

        let Some(handle) = &self.audio else {
            // Cache-only mode: decode succeeded, drop the samples.
            return Ok(());
        };
        let sink = rodio::Sink::try_new(handle).map_err(|e| BundleError::Audio(e.to_string()))?;
        sink.append(rodio::buffer::SamplesBuffer::new(
            channels,
            sample_rate,
            samples,
        ));
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

struct DecodedPcm {
    samples: Vec<i16>,
    sample_rate: u32,
    channels: u16,
}

fn decode_to_pcm(path: &Path) -> Result<DecodedPcm, BundleError> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let format_opts = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let metadata_opts = MetadataOptions::default();
    let probed = symphonia::default::get_probe()
        .format(&Hint::new(), mss, &format_opts, &metadata_opts)
        .map_err(|e| BundleError::Decode(format!("probe: {e}")))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| BundleError::Decode("no usable track".into()))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| BundleError::Decode(format!("codec: {e}")))?;

    let mut samples: Vec<i16> = Vec::new();
    let mut sample_rate: u32 = 0;
    let mut channels: u16 = 0;
    let mut sample_buf: Option<SampleBuffer<i16>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // Symphonia signals end-of-stream as an io::Error; treat any
            // io::Error during demuxing as "we got what we got" rather
            // than fatal, so partial files still play whatever decoded.
            Err(SymphoniaError::IoError(_)) => break,
            Err(e) => return Err(BundleError::Decode(format!("next_packet: {e}"))),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let spec = *audio_buf.spec();
                let buf = sample_buf.get_or_insert_with(|| {
                    sample_rate = spec.rate;
                    channels = spec.channels.count() as u16;
                    SampleBuffer::<i16>::new(audio_buf.capacity() as u64, spec)
                });
                buf.copy_interleaved_ref(audio_buf);
                samples.extend_from_slice(buf.samples());
            }
            // Per symphonia docs, isolated DecodeErrors are recoverable —
            // skip the bad packet and continue.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(BundleError::Decode(format!("decode: {e}"))),
        }
    }

    if samples.is_empty() {
        return Err(BundleError::Decode("no audio samples decoded".into()));
    }
    Ok(DecodedPcm {
        samples,
        sample_rate,
        channels,
    })
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

    // Some upstream TTS providers return AAC-in-MP4 (.m4a) bytes even when
    // the file is named `.mp3`. We probe by magic, not extension, so this
    // exercises the same code path that loads on-disk course bundles.
    #[tokio::test]
    async fn play_with_no_audio_handle_decodes_real_m4a_fixture() {
        let player = BundlePlayer::new(None);
        let res = player.play(Path::new("fixtures/audio/silence.m4a")).await;
        assert!(res.is_ok(), "expected m4a fixture to decode, got {res:?}");
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
        player.cancel();
    }
}
