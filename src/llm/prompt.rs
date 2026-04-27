//! Prompt templates and error rendering for the Reflexion loop.
//!
//! The three string constants are frozen into insta snapshots (see
//! `tests/llm.rs::prompts`). Any edit to a template must be reviewed via the
//! snapshot diff — this protects generation quality from accidental drift.

use std::fmt::Write as _;

use crate::storage::course::ValidationError;

/// Phase 1 system prompt: article → title + description + sentences.
/// Takes user's English level as a parameter to filter appropriate sentences.
pub fn phase1_system(level_description: &str) -> String {
    format!(
        r#"You are a bilingual language tutor preparing a typing-practice lesson from an English article.

The learner's English level is: {level_description}

Output ONLY JSON, no markdown fences, no commentary. Schema:

{{
  "title":       "English string, 1-100 chars, a concise lesson title",
  "description": "Optional Chinese description, ≤300 chars (empty string allowed)",
  "sentences": [
    {{ "chinese": "idiomatic Chinese translation (1-200 chars)",
      "english": "sentence from the article, 5-30 words, self-contained, typable ASCII" }}
  ]
}}

Rules:
- Select 5–20 pedagogically useful sentences appropriate for the learner's level.
- Filter sentences based on the level description above — skip sentences that are too easy or too difficult.
- Prioritize sentences with useful vocabulary and grammar patterns for this level.
- If the article is long, pick the most instructive sentences; do NOT quote the whole article.
- Each English sentence must be typable (ASCII letters, straight quotes, basic punctuation).
- `chinese` MUST be idiomatic, native-feeling Chinese — NOT a word-for-word literal translation:
  * Reorder constituents to match natural Chinese grammar (Chinese is modifier-before-noun; English often modifier-after-noun via clauses or "of"-phrases).
  * Use the natural Chinese term for each concept, not the most direct dictionary gloss. Examples (avoid → prefer):
    – "emergent" → "涌现的" (NOT "出现的")
    – "tool-using" (modifier) → "会使用工具的" (NOT "工具使用")
    – "self-correcting" → "自我修正的" or "能自我修正的" (NOT a noun phrase like "自我纠正")
    – "goal-directed" → "以目标为导向的" or "面向目标的" (NOT "目标导向的" as a noun)
  * If a clause modifies a noun in English (e.g. "the entity the user interacts with"), turn it into a 的-clause before the noun in Chinese (e.g. "用户与之交互的实体").
  * Read your Chinese aloud — if it sounds like translation-ese, rewrite it.
- Return JSON only.
"#
    )
}

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
- Progressive order:
  1. keywords: Core phrase (2-5 words), MUST be a meaningful phrase, NOT a word list. Example: "act toward" not "act, toward"
  2. skeleton: Subject-verb-object core structure
  3. clause (optional): Add one modifier layer
  4. full: Complete sentence
- The LAST drill MUST have focus="full" and its english MUST match the input english verbatim.
- `stage` is 1-indexed and strictly increasing.
- `chinese` is 1-200 chars. `english` is 1-50 words.
- `chinese` MUST be idiomatic, native-feeling Chinese (NOT word-for-word from the english):
  * Reorder freely so it reads naturally in Chinese; English-style postmodifier clauses become 的-phrases before the noun.
  * Use the natural Chinese term, not the direct dictionary gloss. Examples: "emergent" → "涌现的" (NOT "出现的"); "tool-using" (modifier) → "会使用工具的" (NOT "工具使用"); "self-correcting" → "能自我修正的" (NOT "自我纠正").
  * For partial drills (keywords / skeleton / clause), the chinese should be the natural Chinese rendering of the same partial idea — not a mechanical fragment.
- `english` field REQUIREMENTS:
  * Plain English words and basic punctuation ONLY (letters, digits, spaces, `.,;:!?'"()-`)
  * NEVER include IPA symbols (ˈ ˌ ː ə ɒ ɜ ʌ ɪ ʊ ɛ ɔ ɑ æ θ ð ʃ ʒ ŋ, etc.) — those belong solely in `soundmark`
  * NEVER include slash-delimited phonetic transcriptions
- `soundmark` REQUIREMENTS:
  * Must be IPA pronunciation of the ENGLISH text, NOT the Chinese text
  * Use General American (GenAm) pronunciation as the reference dialect
  * ALL stages (including "full") MUST provide IPA wrapped in /slashes/ per word. Example: "/sɛns/ /wɛər/ /juː/ /ɑːr/"
  * One slash group per English word. Each group is ONE continuous IPA string between its slashes — do NOT insert spaces or periods (`.`) inside the slashes (use the primary stress mark `ˈ` to separate syllables, not `.`)
  * Skip punctuation (commas, periods) — punctuation in `english` does NOT get its own slash group
  * Match the standard dictionary form. Common mistakes to AVOID:
    – Dropping final consonants. "user" is /ˈjuːzər/ (NOT /ˈjuːər/); "is" is /ɪz/; "interacts" is /ˌɪntərˈækts/
    – Wrong sibilant suffix. Plural / 3rd-person -s is /s/ after voiceless, /z/ after voiced, /ɪz/ after sibilants (e.g. "needs" → /niːdz/, "watches" → /ˈwɑːtʃɪz/)
    – Wrong past-tense suffix. -ed is /t/ after voiceless, /d/ after voiced, /ɪd/ after t/d
  * NEVER use Chinese pinyin or Chinese phonetics — only English IPA
  * NEVER leave soundmark empty for any stage
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
        assert!(phase1_system("intermediate").contains("5–20"));
    }

    #[test]
    fn phase2_system_mentions_full_last_constraint() {
        assert!(PHASE2_SYSTEM.contains("LAST drill MUST have focus=\"full\""));
    }
}
