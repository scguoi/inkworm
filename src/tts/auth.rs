//! iFlytek WS URL signing (per design spec §7.4).
//!
//! All inputs are explicit (including `now: SystemTime`) so tests can
//! freeze the clock and capture the exact URL via `insta`.

use std::time::SystemTime;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;

const HOST: &str = "tts-api.xfyun.cn";
const REQUEST_LINE: &str = "GET /v2/tts HTTP/1.1";

type HmacSha256 = Hmac<Sha256>;

/// Build the full wss:// URL (with `authorization`, `date`, `host` query params)
/// signed per iFlytek's HMAC-SHA256 scheme.
pub fn build_authorized_url(api_key: &str, api_secret: &str, now: SystemTime) -> String {
    let date = httpdate::fmt_http_date(now);
    let signature_origin = format!("host: {HOST}\ndate: {date}\n{REQUEST_LINE}");
    let sig = hmac_sha256_base64(api_secret.as_bytes(), signature_origin.as_bytes());
    let auth_origin = format!(
        r#"api_key="{api_key}", algorithm="hmac-sha256", headers="host date request-line", signature="{sig}""#
    );
    let auth = BASE64.encode(auth_origin);
    format!(
        "wss://{HOST}/v2/tts?authorization={}&date={}&host={HOST}",
        urlencoding::encode(&auth),
        urlencoding::encode(&date),
    )
}

fn hmac_sha256_base64(key: &[u8], msg: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts any key length");
    mac.update(msg);
    let result = mac.finalize().into_bytes();
    BASE64.encode(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    /// A fixed SystemTime used for deterministic signature tests.
    fn fixed_time() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_776_483_912)
    }

    #[test]
    fn hmac_is_deterministic_for_same_input() {
        let a = hmac_sha256_base64(b"key", b"message");
        let b = hmac_sha256_base64(b"key", b"message");
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn hmac_differs_when_message_differs() {
        let a = hmac_sha256_base64(b"key", b"message A");
        let b = hmac_sha256_base64(b"key", b"message B");
        assert_ne!(a, b);
    }

    #[test]
    fn url_contains_required_query_params() {
        let url = build_authorized_url("test-key", "test-secret", fixed_time());
        assert!(url.starts_with("wss://tts-api.xfyun.cn/v2/tts?"));
        assert!(url.contains("authorization="));
        assert!(url.contains("date="));
        assert!(url.contains("host=tts-api.xfyun.cn"));
    }

    #[test]
    fn authorized_url_snapshot() {
        let url = build_authorized_url("test-key", "test-secret", fixed_time());
        insta::assert_snapshot!(url);
    }
}
