//! TTS cache key derivation and path resolution (per spec §7.1).
//! Cache key = blake3(text || '\n' || voice), hex-encoded.

use std::path::{Path, PathBuf};

/// Derive a stable cache key from text + voice.
/// Uses a newline separator so "abc" + "def" and "ab" + "cdef" hash differently.
pub fn cache_key(text: &str, voice: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(text.as_bytes());
    hasher.update(b"\n");
    hasher.update(voice.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Full path to a cached WAV file, `<dir>/<key>.wav`.
pub fn cache_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.wav"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_produce_same_key() {
        let a = cache_key("hello world", "x3_catherine");
        let b = cache_key("hello world", "x3_catherine");
        assert_eq!(a, b);
    }

    #[test]
    fn different_text_produces_different_key() {
        let a = cache_key("hello", "v");
        let b = cache_key("world", "v");
        assert_ne!(a, b);
    }

    #[test]
    fn different_voice_produces_different_key() {
        let a = cache_key("hello", "voice_a");
        let b = cache_key("hello", "voice_b");
        assert_ne!(a, b);
    }

    #[test]
    fn separator_prevents_concat_collision() {
        // Without the newline separator, "abc" + "def" == "ab" + "cdef" would collide.
        let a = cache_key("abc", "def");
        let b = cache_key("ab", "cdef");
        assert_ne!(a, b);
    }

    #[test]
    fn cache_path_joins_dir_and_key_with_wav_extension() {
        let p = cache_path(Path::new("/tmp/inkworm/tts-cache"), "abc123");
        assert_eq!(p.to_str(), Some("/tmp/inkworm/tts-cache/abc123.wav"));
    }

    #[test]
    fn cache_key_is_hex_64_chars() {
        let k = cache_key("x", "y");
        assert_eq!(k.len(), 64);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
