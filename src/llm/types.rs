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
}
