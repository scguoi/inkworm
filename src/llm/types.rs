//! Wire types for the OpenAI-compatible chat completions API, and the Raw*
//! structs that the Reflexion loop deserializes into before promoting them to
//! validated `storage::course` types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    JsonObject,
}

/// The top-level response shape from `/chat/completions`. We only read the
/// first choice's `message.content`.
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponseMessage {
    pub content: String,
}

use crate::storage::course::Focus;

/// Phase 1 output shape: what the LLM must return when splitting an article.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawSentences {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub sentences: Vec<RawSentence>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawSentence {
    pub chinese: String,
    pub english: String,
}

/// Phase 2 output shape: what the LLM must return when expanding one sentence
/// into drills.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawDrills {
    pub drills: Vec<RawDrill>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawDrill {
    pub stage: u32,
    pub focus: Focus,
    pub chinese: String,
    pub english: String,
    #[serde(default)]
    pub soundmark: String,
}

impl RawSentences {
    /// Collect ALL validation errors (empty Vec = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let title_len = self.title.chars().count();
        if title_len == 0 || title_len > 100 {
            errs.push(format!("title length must be 1..=100, got {title_len}"));
        }
        let desc_len = self.description.chars().count();
        if desc_len > 300 {
            errs.push(format!("description length must be ≤300, got {desc_len}"));
        }
        let n = self.sentences.len();
        if !(5..=20).contains(&n) {
            errs.push(format!("sentences length must be 5..=20, got {n}"));
        }
        for (i, s) in self.sentences.iter().enumerate() {
            let clen = s.chinese.chars().count();
            if !(1..=200).contains(&clen) {
                errs.push(format!(
                    "sentences[{i}].chinese length must be 1..=200, got {clen}"
                ));
            }
            let words = s.english.split_whitespace().count();
            if !(1..=50).contains(&words) {
                errs.push(format!(
                    "sentences[{i}].english word count must be 1..=50, got {words}"
                ));
            }
        }
        errs
    }
}

impl RawDrills {
    /// Collect ALL validation errors. `reference_english` is the Phase 1
    /// english string this drill-set is supposed to expand; the last drill's
    /// english must match it.
    pub fn validate(&self, reference_english: &str) -> Vec<String> {
        let mut errs = Vec::new();
        let n = self.drills.len();
        if !(3..=5).contains(&n) {
            errs.push(format!("drills length must be 3..=5, got {n}"));
        }
        for (j, d) in self.drills.iter().enumerate() {
            let expected_stage = (j as u32) + 1;
            if d.stage != expected_stage {
                errs.push(format!(
                    "drills[{j}].stage must be {expected_stage}, got {}",
                    d.stage
                ));
            }
            let clen = d.chinese.chars().count();
            if !(1..=200).contains(&clen) {
                errs.push(format!(
                    "drills[{j}].chinese length must be 1..=200, got {clen}"
                ));
            }
            let words = d.english.split_whitespace().count();
            if !(1..=50).contains(&words) {
                errs.push(format!(
                    "drills[{j}].english word count must be 1..=50, got {words}"
                ));
            }
        }
        if let Some(last) = self.drills.last() {
            if last.focus != Focus::Full {
                errs.push(format!(
                    "last drill focus must be \"full\", got \"{:?}\"",
                    last.focus
                ));
            }
            if last.english.trim() != reference_english.trim() {
                errs.push(format!(
                    "last drill english must match reference exactly; got {:?}, expected {:?}",
                    last.english, reference_english
                ));
            }
        }
        errs
    }
}

impl ChatRequest {
    pub fn system_and_user(model: impl Into<String>, system: String, user: String) -> Self {
        Self {
            model: model.into(),
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: system,
                },
                ChatMessage {
                    role: Role::User,
                    content: user,
                },
            ],
            temperature: Some(0.3),
            max_tokens: None,
            response_format: Some(ResponseFormat::JsonObject),
        }
    }

    /// Append an assistant message and a user "repair" message, preserving the
    /// existing conversation history (Reflexion cumulative context).
    pub fn append_repair(&mut self, prior_assistant: String, repair: String) {
        self.messages.push(ChatMessage {
            role: Role::Assistant,
            content: prior_assistant,
        });
        self.messages.push(ChatMessage {
            role: Role::User,
            content: repair,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_and_user_constructs_two_messages() {
        let req = ChatRequest::system_and_user("m", "sys".into(), "usr".into());
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[1].role, Role::User);
    }

    #[test]
    fn append_repair_grows_history() {
        let mut req = ChatRequest::system_and_user("m", "sys".into(), "usr".into());
        req.append_repair("bad json".into(), "fix it".into());
        assert_eq!(req.messages.len(), 4);
        assert_eq!(req.messages[2].role, Role::Assistant);
        assert_eq!(req.messages[3].role, Role::User);
    }

    #[test]
    fn role_serializes_lowercase() {
        let s = serde_json::to_string(&Role::System).unwrap();
        assert_eq!(s, "\"system\"");
    }

    #[test]
    fn request_skips_none_options() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![],
            temperature: None,
            max_tokens: None,
            response_format: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        assert!(!s.contains("temperature"));
        assert!(!s.contains("max_tokens"));
        assert!(!s.contains("response_format"));
    }

    #[test]
    fn raw_sentences_minimum_valid() {
        let rs = RawSentences {
            title: "T".into(),
            description: "".into(),
            sentences: (0..5)
                .map(|_| RawSentence {
                    chinese: "中".into(),
                    english: "two words here".into(),
                })
                .collect(),
        };
        assert!(rs.validate().is_empty());
    }

    #[test]
    fn raw_sentences_too_few_flagged() {
        let rs = RawSentences {
            title: "T".into(),
            description: "".into(),
            sentences: (0..4)
                .map(|_| RawSentence {
                    chinese: "中".into(),
                    english: "two words".into(),
                })
                .collect(),
        };
        let errs = rs.validate();
        assert!(errs.iter().any(|e| e.contains("sentences length")));
    }

    #[test]
    fn raw_drills_last_full_required() {
        use crate::storage::course::Focus;
        let rd = RawDrills {
            drills: vec![
                RawDrill {
                    stage: 1,
                    focus: Focus::Keywords,
                    chinese: "中".into(),
                    english: "one two".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 2,
                    focus: Focus::Skeleton,
                    chinese: "中".into(),
                    english: "one two three".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 3,
                    focus: Focus::Clause,
                    chinese: "中".into(),
                    english: "one two three four".into(),
                    soundmark: "".into(),
                },
            ],
        };
        let errs = rd.validate("one two three four");
        assert!(
            errs.iter().any(|e| e.contains("last drill focus")),
            "{errs:#?}"
        );
    }

    #[test]
    fn raw_drills_english_mismatch_flagged() {
        use crate::storage::course::Focus;
        let rd = RawDrills {
            drills: vec![
                RawDrill {
                    stage: 1,
                    focus: Focus::Keywords,
                    chinese: "中".into(),
                    english: "a b".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 2,
                    focus: Focus::Skeleton,
                    chinese: "中".into(),
                    english: "a b c".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 3,
                    focus: Focus::Full,
                    chinese: "中".into(),
                    english: "different sentence here".into(),
                    soundmark: "".into(),
                },
            ],
        };
        let errs = rd.validate("expected reference sentence");
        assert!(errs.iter().any(|e| e.contains("match reference")));
    }

    #[test]
    fn raw_drills_minimum_valid() {
        use crate::storage::course::Focus;
        let rd = RawDrills {
            drills: vec![
                RawDrill {
                    stage: 1,
                    focus: Focus::Keywords,
                    chinese: "中".into(),
                    english: "a b".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 2,
                    focus: Focus::Skeleton,
                    chinese: "中".into(),
                    english: "a b c".into(),
                    soundmark: "".into(),
                },
                RawDrill {
                    stage: 3,
                    focus: Focus::Full,
                    chinese: "中".into(),
                    english: "exact ref".into(),
                    soundmark: "".into(),
                },
            ],
        };
        assert!(rd.validate("exact ref").is_empty());
    }
}
