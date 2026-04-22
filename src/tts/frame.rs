//! iFlytek TTS online WS request/response frames (per developer docs).
//! Request is one-shot (`data.status = 2`); response is streamed with
//! `data.status` 0 (first chunk), 1 (continuing), 2 (final chunk).

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct RequestFrame<'a> {
    pub common: RequestCommon<'a>,
    pub business: RequestBusiness<'a>,
    pub data: RequestData,
}

#[derive(Debug, Serialize)]
pub struct RequestCommon<'a> {
    pub app_id: &'a str,
}

#[derive(Debug, Serialize)]
pub struct RequestBusiness<'a> {
    pub aue: &'a str, // "raw" = 16kHz 16-bit mono PCM
    pub vcn: &'a str, // voice code, e.g. "x3_catherine"
    pub tte: &'a str, // text encoding, "UTF8"
}

#[derive(Debug, Serialize)]
pub struct RequestData {
    pub status: u8,   // 2 = only-and-final frame
    pub text: String, // base64-encoded UTF-8 text
}

/// Build the one-shot request frame JSON string for the given text + voice + app id.
pub fn build_request_frame(app_id: &str, voice: &str, text: &str) -> String {
    let frame = RequestFrame {
        common: RequestCommon { app_id },
        business: RequestBusiness {
            aue: "raw",
            vcn: voice,
            tte: "UTF8",
        },
        data: RequestData {
            status: 2,
            text: BASE64.encode(text.as_bytes()),
        },
    };
    serde_json::to_string(&frame).expect("RequestFrame always serializes")
}

#[derive(Debug, Deserialize)]
pub struct ResponseFrame {
    pub code: i32,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub sid: String,
    pub data: Option<ResponseData>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseData {
    pub status: u8, // 0=first, 1=continuing, 2=final
    #[serde(default)]
    pub audio: String, // base64 PCM chunk (may be empty on some frames)
    #[serde(default)]
    pub ced: String, // opaque progress counter (ignored)
}

impl ResponseFrame {
    pub fn is_ok(&self) -> bool {
        self.code == 0
    }
    pub fn is_final(&self) -> bool {
        matches!(self.data.as_ref().map(|d| d.status), Some(2))
    }
}

/// Decode a response frame's base64 `audio` field into raw i16 PCM samples (LE).
/// Returns `Ok(vec![])` for empty audio payloads (some frames carry no audio).
pub fn decode_pcm(b64: &str) -> Result<Vec<i16>, FrameError> {
    if b64.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = BASE64
        .decode(b64)
        .map_err(|e| FrameError::Base64(e.to_string()))?;
    if bytes.len() % 2 != 0 {
        return Err(FrameError::Base64(format!(
            "odd byte count: {}",
            bytes.len()
        )));
    }
    let mut samples = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        samples.push(i16::from_le_bytes([pair[0], pair[1]]));
    }
    Ok(samples)
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("base64 decode: {0}")]
    Base64(String),
    #[error("json: {0}")]
    Json(String),
}

/// Parse a raw WS text-frame payload into a `ResponseFrame`.
pub fn parse_response(text: &str) -> Result<ResponseFrame, FrameError> {
    serde_json::from_str(text).map_err(|e| FrameError::Json(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_frame_is_valid_json_with_base64_text() {
        let raw = build_request_frame("app-xxx", "x3_catherine", "Hello!");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["common"]["app_id"], "app-xxx");
        assert_eq!(v["business"]["aue"], "raw");
        assert_eq!(v["business"]["vcn"], "x3_catherine");
        assert_eq!(v["business"]["tte"], "UTF8");
        assert_eq!(v["data"]["status"], 2);
        let b64 = v["data"]["text"].as_str().unwrap();
        let decoded = BASE64.decode(b64).unwrap();
        assert_eq!(decoded, b"Hello!");
    }

    #[test]
    fn parse_response_ok_with_final_flag() {
        let raw =
            r#"{"code":0,"message":"success","sid":"x","data":{"status":2,"audio":"","ced":"0"}}"#;
        let f = parse_response(raw).unwrap();
        assert!(f.is_ok());
        assert!(f.is_final());
    }

    #[test]
    fn parse_response_error_code_surfaces() {
        let raw = r#"{"code":10105,"message":"auth failed","sid":"x"}"#;
        let f = parse_response(raw).unwrap();
        assert!(!f.is_ok());
        assert_eq!(f.code, 10105);
        assert_eq!(f.message, "auth failed");
        assert!(f.data.is_none());
    }

    #[test]
    fn decode_pcm_empty_returns_empty() {
        assert_eq!(decode_pcm("").unwrap(), Vec::<i16>::new());
    }

    #[test]
    fn decode_pcm_round_trip() {
        let samples: Vec<i16> = vec![0, 256, -256, 32767, -32768];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let b64 = BASE64.encode(bytes);
        let decoded = decode_pcm(&b64).unwrap();
        assert_eq!(decoded, samples);
    }

    #[test]
    fn decode_pcm_odd_byte_count_is_error() {
        // "AQID" base64 decodes to 3 bytes — odd length.
        let err = decode_pcm("AQID").unwrap_err();
        assert!(matches!(err, FrameError::Base64(_)));
    }
}
