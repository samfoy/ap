use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

pub mod bedrock;

pub use bedrock::BedrockProvider;

// ─── Message types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// A single unit of content inside a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// A conversation message (user or assistant turn).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<MessageContent>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![MessageContent::Text { text: text.into() }],
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![MessageContent::Text { text: text.into() }],
        }
    }
}

// ─── Stream events ────────────────────────────────────────────────────────────

/// Events emitted by the provider stream.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of text from the assistant.
    TextDelta(String),
    /// A tool call is starting.
    ToolUseStart { id: String, name: String },
    /// A JSON fragment of tool call parameters (accumulated into a full JSON string).
    ToolUseParams(String),
    /// The tool call parameters are complete.
    ToolUseEnd,
    /// The turn is complete.
    TurnEnd {
        stop_reason: String,
        input_tokens: u32,
        output_tokens: u32,
    },
}

// ─── Provider error ──────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("AWS error: {0}")]
    Aws(String),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

// ─── Provider trait ──────────────────────────────────────────────────────────

/// A streaming LLM provider. Object-safe via `BoxStream`.
pub trait Provider: Send + Sync {
    fn stream_completion<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [serde_json::Value],
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>>;
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_event_variants() {
        // Verify each StreamEvent variant can be constructed and is debuggable.
        let text_delta = StreamEvent::TextDelta("hello".to_string());
        assert!(format!("{:?}", text_delta).contains("hello"));

        let tool_start = StreamEvent::ToolUseStart {
            id: "tool_1".to_string(),
            name: "bash".to_string(),
        };
        assert!(format!("{:?}", tool_start).contains("bash"));

        let params = StreamEvent::ToolUseParams(r#"{"cmd":"ls"}"#.to_string());
        assert!(format!("{:?}", params).contains("cmd"));

        let tool_end = StreamEvent::ToolUseEnd;
        assert!(format!("{:?}", tool_end).contains("ToolUseEnd"));

        let turn_end = StreamEvent::TurnEnd {
            stop_reason: "end_turn".to_string(),
            input_tokens: 10,
            output_tokens: 20,
        };
        assert!(format!("{:?}", turn_end).contains("end_turn"));
        assert!(format!("{:?}", turn_end).contains("10"));
    }

    #[test]
    fn test_provider_error_display() {
        let err = ProviderError::Aws("connection failed".into());
        assert!(err.to_string().contains("connection failed"));

        let parse_err = ProviderError::ParseError("unexpected token".into());
        assert!(parse_err.to_string().contains("unexpected token"));

        let json_err: Result<serde_json::Value, _> = serde_json::from_str("{bad}");
        let provider_err = ProviderError::Serialization(json_err.unwrap_err());
        assert!(!provider_err.to_string().is_empty());
    }

    #[test]
    fn test_message_constructors() {
        let user_msg = Message::user("Hello");
        assert_eq!(user_msg.role, Role::User);
        assert_eq!(
            user_msg.content,
            vec![MessageContent::Text {
                text: "Hello".to_string()
            }]
        );

        let asst_msg = Message::assistant("World");
        assert_eq!(asst_msg.role, Role::Assistant);
    }
}
