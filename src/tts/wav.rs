//! WAV file I/O for the TTS cache.
//! Format: mono, 16-bit PCM, 16_000 Hz — matches iFlytek's `aue=raw` output.

use std::fs::{self, File};
use std::io;
use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};

pub const SAMPLE_RATE: u32 = 16_000;
pub const CHANNELS: u16 = 1;
pub const BITS_PER_SAMPLE: u16 = 16;

fn spec() -> WavSpec {
    WavSpec {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: BITS_PER_SAMPLE,
        sample_format: SampleFormat::Int,
    }
}

/// Write `samples` (i16 PCM, mono 16kHz) to `path` via a temp-file + rename dance.
/// The destination directory must already exist.
pub fn write_wav_atomic(path: &Path, samples: &[i16]) -> io::Result<()> {
    let tmp_path = path.with_extension("wav.tmp");
    {
        let tmp_file = File::create(&tmp_path)?;
        let mut writer = WavWriter::new(tmp_file, spec())
            .map_err(|e| io::Error::other(format!("wav header: {e}")))?;
        for s in samples {
            writer
                .write_sample(*s)
                .map_err(|e| io::Error::other(format!("wav sample: {e}")))?;
        }
        writer
            .finalize()
            .map_err(|e| io::Error::other(format!("wav finalize: {e}")))?;
    }
    // Re-open the finalized tmp file by path to fsync its contents before
    // rename (mirroring storage::atomic::write_atomic semantics).
    File::open(&tmp_path)?.sync_all()?;
    fs::rename(&tmp_path, path)?;
    if let Some(parent) = path.parent() {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Read a previously-written WAV file back into an i16 PCM buffer.
/// Returns an error if the format does not match our fixed spec.
pub fn read_wav_pcm(path: &Path) -> io::Result<Vec<i16>> {
    let reader =
        hound::WavReader::open(path).map_err(|e| io::Error::other(format!("wav open: {e}")))?;
    let header = reader.spec();
    if header.channels != CHANNELS
        || header.sample_rate != SAMPLE_RATE
        || header.bits_per_sample != BITS_PER_SAMPLE
        || header.sample_format != SampleFormat::Int
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unexpected wav spec: {:?} (want {}ch {}Hz {}-bit int)",
                header, CHANNELS, SAMPLE_RATE, BITS_PER_SAMPLE
            ),
        ));
    }
    let mut reader = reader;
    reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io::Error::other(format!("wav sample read: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_samples() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.wav");
        let samples: Vec<i16> = (0..1000).map(|i| (i as i16).wrapping_mul(23)).collect();

        write_wav_atomic(&path, &samples).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("wav.tmp").exists());

        let got = read_wav_pcm(&path).unwrap();
        assert_eq!(got, samples);
    }

    #[test]
    fn write_produces_nonempty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.wav");
        write_wav_atomic(&path, &[0, 0, 0]).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() >= 44 + 6);
    }

    #[test]
    fn read_wav_rejects_unexpected_spec() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        {
            let file = File::create(&path).unwrap();
            let mut writer = WavWriter::new(file, spec).unwrap();
            writer.write_sample(0i16).unwrap();
            writer.write_sample(0i16).unwrap();
            writer.finalize().unwrap();
        }
        let err = read_wav_pcm(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn write_overwrites_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("o.wav");
        write_wav_atomic(&path, &[1, 2, 3]).unwrap();
        write_wav_atomic(&path, &[9, 8, 7, 6]).unwrap();
        let got = read_wav_pcm(&path).unwrap();
        assert_eq!(got, vec![9, 8, 7, 6]);
    }
}
