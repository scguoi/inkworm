//! Persists LLM responses that failed three repair attempts, so users can
//! inspect them post-mortem and we never pollute the courses library.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::storage::course::StorageError;

/// One failed attempt's raw LLM output + validation errors.
#[derive(Debug, Clone)]
pub struct AttemptFailure {
    pub attempt_number: u32,
    pub raw: String,
    pub errors: Vec<String>,
}

/// Write a human-readable failure report and return the path written.
///
/// `phase` is 1 (article split) or 2 (single-sentence drill expansion).
/// `sentence_index` applies only to phase 2, identifying which sentence
/// failed.
pub fn save_failed_response(
    failed_dir: &Path,
    now: DateTime<Utc>,
    phase: u8,
    sentence_index: Option<usize>,
    model: &str,
    input_preview: &str,
    attempts: &[AttemptFailure],
) -> Result<PathBuf, StorageError> {
    std::fs::create_dir_all(failed_dir)?;
    let ts = now.format("%Y-%m-%d-%H-%M-%S");
    let suffix = match (phase, sentence_index) {
        (1, _) => format!("{ts}-phase1.txt"),
        (2, Some(i)) => format!("{ts}-phase2-s{i}.txt"),
        _ => format!("{ts}-phase{phase}.txt"),
    };
    let path = failed_dir.join(suffix);

    let mut body = String::new();
    body.push_str("=== inkworm reflexion failure ===\n");
    body.push_str(&format!("timestamp: {}\n", now.to_rfc3339()));
    body.push_str(&format!("phase: {phase}\n"));
    if let Some(i) = sentence_index {
        body.push_str(&format!("sentence_index: {i}\n"));
    }
    body.push_str(&format!("model: {model}\n"));
    body.push_str("\ninput (truncated to 500 chars):\n");
    let input_cut: String = input_preview.chars().take(500).collect();
    body.push_str(&input_cut);
    body.push('\n');
    for a in attempts {
        body.push_str(&format!("\n--- attempt {} ---\nraw:\n", a.attempt_number));
        body.push_str(&a.raw);
        body.push_str("\nerrors:\n");
        for e in &a.errors {
            body.push_str(&format!("- {e}\n"));
        }
    }

    // Plain-text file; atomic write not strictly required (stale partials are
    // harmless), but use it for consistency.
    crate::storage::atomic::write_atomic(&path, body.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::tempdir;

    fn fixed() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 21, 10, 30, 42).unwrap()
    }

    #[test]
    fn writes_phase1_file_with_expected_name() {
        let t = tempdir().unwrap();
        let path = save_failed_response(
            t.path(),
            fixed(),
            1,
            None,
            "gpt-4o-mini",
            "some article text",
            &[AttemptFailure {
                attempt_number: 1,
                raw: "bad".into(),
                errors: vec!["missing title".into()],
            }],
        )
        .unwrap();
        assert!(path.ends_with("2026-04-21-10-30-42-phase1.txt"));
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("missing title"));
        assert!(body.contains("phase: 1"));
    }

    #[test]
    fn writes_phase2_file_with_sentence_index() {
        let t = tempdir().unwrap();
        let path = save_failed_response(
            t.path(),
            fixed(),
            2,
            Some(7),
            "gpt-4o-mini",
            "sentence",
            &[],
        )
        .unwrap();
        assert!(path.ends_with("2026-04-21-10-30-42-phase2-s7.txt"));
    }

    #[test]
    fn truncates_long_input_preview() {
        let t = tempdir().unwrap();
        let long = "x".repeat(1000);
        let path = save_failed_response(t.path(), fixed(), 1, None, "m", &long, &[]).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        // Only 500 `x` should appear after the "input" header.
        let after = body.split("truncated to 500 chars):\n").nth(1).unwrap();
        let first_line = after.lines().next().unwrap();
        assert_eq!(first_line.len(), 500);
    }
}
