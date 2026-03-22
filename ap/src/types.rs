// src/types.rs — Core data types for the FP refactor.
//
// This module defines the immutable `Conversation` value, `TurnEvent` for
// streaming pipeline output, `ToolCall` for tool-use requests, and the
// `Middleware` chain types.

use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::provider::{Message, MessageContent, Role};
use crate::tools::ToolResult;

// ─── Conversation ─────────────────────────────────────────────────────────────

/// Immutable conversation value — each turn returns a new `Conversation`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub config: AppConfig,
    /// Transient system prompt injected by skill middleware each turn.
    /// Not persisted to session files — skills are re-scored on every turn.
    #[serde(skip)]
    pub system_prompt: Option<String>,
}

impl Conversation {
    pub fn new(
        id: impl Into<String>,
        model: impl Into<String>,
        config: AppConfig,
    ) -> Self {
        Self {
            id: id.into(),
            model: model.into(),
            messages: Vec::new(),
            config,
            system_prompt: None,
        }
    }

    /// Return a new `Conversation` with a user message appended.
    /// The original is consumed (not mutated) — caller gets new value.
    pub fn with_user_message(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message {
            role: Role::User,
            content: vec![MessageContent::Text { text: content.into() }],
        });
        self
    }

    /// Return a new `Conversation` with the given system prompt set.
    ///
    /// The `system_prompt` field is transient (`#[serde(skip)]`) — it is never
    /// written to session files and must be re-injected on every turn by the
    /// skill injection middleware.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Return a new `Conversation` with the messages replaced by the given list.
    /// All other fields (id, model, config, system_prompt) are preserved.
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }
}

// ─── TurnEvent ────────────────────────────────────────────────────────────────

/// Events emitted by the `turn()` pipeline — consumed by both TUI and headless.
#[derive(Debug, Clone)]
pub enum TurnEvent {
    /// A streamed text fragment from the assistant.
    TextChunk(String),
    /// A tool call is about to execute.
    ToolStart {
        name: String,
        params: serde_json::Value,
    },
    /// A tool call completed.
    ToolComplete {
        name: String,
        result: String,
        is_error: bool,
    },
    /// The full agent turn is finished (no more tool calls pending).
    TurnEnd,
    /// Token usage reported at the end of a turn.
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    /// An unrecoverable error occurred.
    Error(String),
    /// Context was compressed — some leading messages replaced by a summary.
    ContextSummarized {
        messages_before: usize,
        messages_after: usize,
        tokens_before: u32,
        tokens_after: u32,
    },
}

// ─── ToolCall ─────────────────────────────────────────────────────────────────

/// A single tool invocation requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique tool-use ID (from the LLM response — needed for the result message).
    pub id: String,
    pub name: String,
    pub params: serde_json::Value,
}

// ─── ToolMiddlewareResult ─────────────────────────────────────────────────────

/// Result from a pre-tool or post-tool middleware function.
#[derive(Debug)]
pub enum ToolMiddlewareResult {
    /// Pass the tool call through (possibly with modifications).
    Allow(ToolCall),
    /// Cancel execution — the given reason is returned to Claude as the result.
    Block(String),
    /// Skip execution entirely, returning the provided result directly.
    Transform(ToolResult),
}

// ─── Middleware ───────────────────────────────────────────────────────────────

/// Newtype aliases so the middleware chain types are readable.
pub type ToolMiddlewareFn =
    Box<dyn Fn(ToolCall) -> ToolMiddlewareResult + Send + Sync>;
pub type TurnMiddlewareFn =
    Box<dyn Fn(&Conversation) -> Option<Conversation> + Send + Sync>;

/// Composable middleware chains for the `turn()` pipeline.
///
/// Each list is applied in order. Builder methods live in `middleware.rs`.
pub struct Middleware {
    pub pre_turn: Vec<TurnMiddlewareFn>,
    pub post_turn: Vec<TurnMiddlewareFn>,
    pub pre_tool: Vec<ToolMiddlewareFn>,
    pub post_tool: Vec<ToolMiddlewareFn>,
}

#[allow(clippy::derivable_impls)]
impl Default for Middleware {
    fn default() -> Self {
        Self {
            pre_turn: Vec::new(),
            post_turn: Vec::new(),
            pre_tool: Vec::new(),
            post_tool: Vec::new(),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dummy_config() -> AppConfig {
        AppConfig::default()
    }

    // AC1: Conversation is immutable-friendly
    #[test]
    fn conversation_with_user_message_appends() {
        let conv = Conversation::new("test-id", "claude-3", dummy_config());
        assert_eq!(conv.messages.len(), 0);

        let conv2 = conv.clone().with_user_message("hello");
        assert_eq!(conv2.messages.len(), 1);

        // Original clone is unchanged — original was consumed, so clone shows no change
        assert_eq!(conv.messages.len(), 0);

        match &conv2.messages[0].content[0] {
            MessageContent::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected text message"),
        }
    }

    // AC2: TurnEvent variants are clonable
    #[test]
    fn turn_event_variants_are_clonable() {
        let events = [
            TurnEvent::TextChunk("hi".to_string()),
            TurnEvent::ToolStart {
                name: "bash".to_string(),
                params: json!({"cmd": "ls"}),
            },
            TurnEvent::ToolComplete {
                name: "bash".to_string(),
                result: "file.txt".to_string(),
                is_error: false,
            },
            TurnEvent::TurnEnd,
            TurnEvent::Usage { input_tokens: 10, output_tokens: 20 },
            TurnEvent::Error("oops".to_string()),
        ];

        let cloned: Vec<TurnEvent> = events.to_vec();
        assert_eq!(cloned.len(), 6);

        // Spot-check cloned values
        if let TurnEvent::TextChunk(ref s) = cloned[0] {
            assert_eq!(s, "hi");
        } else {
            panic!("expected TextChunk");
        }
        if let TurnEvent::TurnEnd = cloned[3] {
        } else {
            panic!("expected TurnEnd");
        }
        if let TurnEvent::Usage { input_tokens, output_tokens } = cloned[4] {
            assert_eq!(input_tokens, 10);
            assert_eq!(output_tokens, 20);
        } else {
            panic!("expected Usage");
        }
    }

    // AC-Step4-1: ContextSummarized is clonable and fields roundtrip
    #[test]
    fn turn_event_context_summarized_clonable() {
        let event = TurnEvent::ContextSummarized {
            messages_before: 10,
            messages_after: 3,
            tokens_before: 5000,
            tokens_after: 500,
        };
        let cloned = event.clone();
        if let TurnEvent::ContextSummarized {
            messages_before,
            messages_after,
            tokens_before,
            tokens_after,
        } = cloned
        {
            assert_eq!(messages_before, 10);
            assert_eq!(messages_after, 3);
            assert_eq!(tokens_before, 5000);
            assert_eq!(tokens_after, 500);
        } else {
            panic!("expected ContextSummarized");
        }
    }

    // AC3: ToolCall roundtrips through serde
    #[test]
    fn tool_call_roundtrip_serde() {
        let call = ToolCall {
            id: "1".to_string(),
            name: "bash".to_string(),
            params: json!({"cmd": "ls"}),
        };
        let json = serde_json::to_string(&call).expect("serialize");
        let back: ToolCall = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, call.id);
        assert_eq!(back.name, call.name);
        assert_eq!(back.params, call.params);
    }

    // AC4: ToolMiddlewareResult has all three variants
    #[test]
    fn tool_middleware_result_variants() {
        let call = ToolCall {
            id: "x".to_string(),
            name: "read".to_string(),
            params: json!({}),
        };

        let allow = ToolMiddlewareResult::Allow(call.clone());
        let block = ToolMiddlewareResult::Block("nope".to_string());
        let transform = ToolMiddlewareResult::Transform(ToolResult::ok("cached"));

        match allow {
            ToolMiddlewareResult::Allow(c) => assert_eq!(c.name, "read"),
            _ => panic!("expected Allow"),
        }
        match block {
            ToolMiddlewareResult::Block(msg) => assert_eq!(msg, "nope"),
            _ => panic!("expected Block"),
        }
        match transform {
            ToolMiddlewareResult::Transform(r) => assert!(!r.is_error),
            _ => panic!("expected Transform"),
        }
    }

    // Bonus: Conversation::new starts with empty messages
    #[test]
    fn conversation_new_has_empty_messages() {
        let conv = Conversation::new("id-1", "model-a", dummy_config());
        assert_eq!(conv.id, "id-1");
        assert_eq!(conv.model, "model-a");
        assert!(conv.messages.is_empty());
    }

    // AC-Step1-1: with_system_prompt sets the field
    #[test]
    fn conversation_with_system_prompt_sets_field() {
        let conv = Conversation::new("id-1", "model-a", dummy_config())
            .with_system_prompt("be helpful");
        assert_eq!(conv.system_prompt, Some("be helpful".to_string()));
    }

    // AC-Step1-2: system_prompt is None by default
    #[test]
    fn conversation_system_prompt_none_by_default() {
        let conv = Conversation::new("id-1", "model-a", dummy_config());
        assert!(conv.system_prompt.is_none());
    }

    // AC-Step1-3: system_prompt is skipped in serde round-trip
    #[test]
    fn conversation_system_prompt_not_serialized() {
        let conv = Conversation::new("id-1", "model-a", dummy_config())
            .with_system_prompt("secret skills");
        let json = serde_json::to_string(&conv).expect("serialize");
        assert!(!json.contains("secret skills"), "system_prompt should not appear in JSON");
        let back: Conversation = serde_json::from_str(&json).expect("deserialize");
        assert!(back.system_prompt.is_none(), "deserialized system_prompt must be None");
    }

    // AC (step-04): old JSON without system_prompt deserializes successfully (backward compat)
    #[test]
    fn conversation_serde_backward_compat() {
        let old_json = r#"{
            "id": "old-id",
            "model": "claude-3",
            "messages": [],
            "config": {}
        }"#;
        let conv: Conversation = serde_json::from_str(old_json).expect("should deserialize");
        assert_eq!(conv.id, "old-id");
        assert_eq!(conv.system_prompt, None);
    }
}
