//! TTS cache key derivation and path resolution (per spec §7.1).
//! Cache key = blake3(text || '\n' || voice_id || '\n' || model), hex-encoded.

use std::path::{Path, PathBuf};

/// Derive a stable cache key from text, voice id, and model.
/// Uses newline separators so "abc" + "def" and "ab" + "cdef" hash differently.
pub fn cache_key(text: &str, voice_id: &str, model: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(text.as_bytes());
    hasher.update(b"\n");
    hasher.update(voice_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(model.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Full path to a cached MP3 file, `<dir>/<key>.mp3`.
pub fn cache_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.mp3"))
}

/// Count `.mp3` files and sum their sizes in `dir`.
/// Returns `(0, 0)` if the directory is missing or unreadable.
pub fn cache_stats(dir: &Path) -> (usize, u64) {
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return (0, 0),
    };
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mp3") {
            continue;
        }
        if let Ok(meta) = path.metadata() {
            if meta.is_file() {
                count += 1;
                bytes += meta.len();
            }
        }
    }
    (count, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_produce_same_key() {
        let a = cache_key("hello world", "x3_catherine", "model_v1");
        let b = cache_key("hello world", "x3_catherine", "model_v1");
        assert_eq!(a, b);
    }

    #[test]
    fn different_text_produces_different_key() {
        let a = cache_key("hello", "v", "m");
        let b = cache_key("world", "v", "m");
        assert_ne!(a, b);
    }

    #[test]
    fn different_voice_produces_different_key() {
        let a = cache_key("hello", "voice_a", "m");
        let b = cache_key("hello", "voice_b", "m");
        assert_ne!(a, b);
    }

    #[test]
    fn different_model_produces_different_key() {
        let a = cache_key("hello", "v", "model_a");
        let b = cache_key("hello", "v", "model_b");
        assert_ne!(a, b);
    }

    #[test]
    fn separator_prevents_concat_collision() {
        // Without the newline separator, "abc" + "def" == "ab" + "cdef" would collide.
        let a = cache_key("abc", "def", "m");
        let b = cache_key("ab", "cdef", "m");
        assert_ne!(a, b);
    }

    #[test]
    fn cache_path_uses_mp3_extension() {
        let p = cache_path(Path::new("/tmp/inkworm/tts-cache"), "abc");
        assert_eq!(p.to_str(), Some("/tmp/inkworm/tts-cache/abc.mp3"));
    }

    #[test]
    fn cache_key_is_hex_64_chars() {
        let k = cache_key("x", "y", "z");
        assert_eq!(k.len(), 64);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn cache_stats_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let (count, bytes) = super::cache_stats(tmp.path());
        assert_eq!(count, 0);
        assert_eq!(bytes, 0);
    }

    #[test]
    fn cache_stats_counts_mp3_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.mp3"), [0u8; 100]).unwrap();
        std::fs::write(tmp.path().join("b.mp3"), [0u8; 200]).unwrap();
        std::fs::write(tmp.path().join("c.txt"), [0u8; 999]).unwrap();
        let (count, bytes) = super::cache_stats(tmp.path());
        assert_eq!(count, 2);
        assert_eq!(bytes, 300);
    }

    #[test]
    fn cache_stats_missing_dir_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        let (count, bytes) = super::cache_stats(&missing);
        assert_eq!(count, 0);
        assert_eq!(bytes, 0);
    }
}
