//! Prompt templates and error rendering for the Reflexion loop.
//!
//! The three string constants are frozen into insta snapshots (see
//! `tests/llm.rs::prompts`). Any edit to a template must be reviewed via the
//! snapshot diff — this protects generation quality from accidental drift.

use std::fmt::Write as _;

use crate::storage::course::ValidationError;

/// Phase 1 system prompt: article → title + description + sentences.
pub const PHASE1_SYSTEM: &str = r#"You are a bilingual language tutor preparing a typing-practice lesson from an English article.

Output ONLY JSON, no markdown fences, no commentary. Schema:

{
  "title":       "English string, 1-100 chars, a concise lesson title",
  "description": "Optional Chinese description, ≤300 chars (empty string allowed)",
  "sentences": [
    { "chinese": "natural Chinese translation (1-200 chars)",
      "english": "sentence from the article, 5-30 words, self-contained, typable ASCII" }
  ]
}

Rules:
- Select 5–20 pedagogically useful sentences (varied grammar, common phrasing).
- If the article is long, pick the most instructive sentences; do NOT quote the whole article.
- Each English sentence must be typable (ASCII letters, straight quotes, basic punctuation).
- Return JSON only.
"#;

/// Phase 2 system prompt: one sentence → 3–5 progressive drills.
pub const PHASE2_SYSTEM: &str = r#"You are a bilingual language tutor decomposing a single sentence into 3–5 progressive typing drills.

Input will be a JSON object { "chinese": "...", "english": "..." }.
Output ONLY JSON, no fences, no commentary. Schema:

{
  "drills": [
    { "stage": 1, "focus": "keywords", "chinese": "...", "english": "...", "soundmark": "IPA or empty string" }
  ]
}

Rules:
- Produce 3 to 5 drills from easy to hard.
- Valid `focus` values: "keywords" | "skeleton" | "clause" | "full".
- Order must progress: keywords (1–5 key words), then skeleton (subject-verb-object core), optionally clause (one modifier layer), and a final "full" stage.
- The LAST drill MUST have focus="full" and its english MUST match the input english verbatim.
- `stage` is 1-indexed and strictly increasing.
- `chinese` is 1-200 chars. `english` is 1-50 words. `soundmark` is IPA wrapped in /slashes/ per word, or an empty string.
- Return JSON only.
"#;

/// Appended to the conversation when a previous attempt failed validation.
/// `{errors}` placeholder is filled at runtime.
pub const REPAIR_TEMPLATE: &str = "Your previous response did not satisfy the schema. Errors:\n{errors}\nReturn ONLY the corrected JSON — same schema, no commentary.";

/// Render a bullet list of validation errors suitable for the repair prompt.
pub fn errors_formatted(errors: &[String]) -> String {
    let mut out = String::new();
    for e in errors {
        let _ = writeln!(out, "- {e}");
    }
    out
}

/// Convenience: render a list of `ValidationError`s (course-level) as bullets.
pub fn course_errors_formatted(errors: &[ValidationError]) -> String {
    errors_formatted(
        &errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<String>>(),
    )
}

/// Build the full repair user message by substituting `{errors}` in the template.
pub fn repair_message(errors: &[String]) -> String {
    REPAIR_TEMPLATE.replace("{errors}", errors_formatted(errors).trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_formatted_produces_bullet_list() {
        let s = errors_formatted(&["a".into(), "b".into()]);
        assert_eq!(s, "- a\n- b\n");
    }

    #[test]
    fn errors_formatted_empty_is_empty() {
        assert_eq!(errors_formatted(&[]), "");
    }

    #[test]
    fn repair_message_substitutes_placeholder() {
        let s = repair_message(&["missing title".into()]);
        assert!(s.contains("missing title"));
        assert!(!s.contains("{errors}"));
        assert!(s.starts_with("Your previous response"));
    }

    #[test]
    fn phase1_system_mentions_sentences_range() {
        assert!(PHASE1_SYSTEM.contains("5–20"));
    }

    #[test]
    fn phase2_system_mentions_full_last_constraint() {
        assert!(PHASE2_SYSTEM.contains("LAST drill MUST have focus=\"full\""));
    }
}
