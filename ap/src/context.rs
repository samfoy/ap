use anyhow::Result;

use crate::config::ContextConfig;
use crate::provider::{Message, MessageContent, Provider, Role, StreamEvent};
use crate::types::{Conversation, TurnEvent};

/// Estimate the number of tokens in a single message using a character-count heuristic.
/// Each content block contributes `chars / 4` tokens, with a minimum of 1.
/// The overall message contributes at least 1 token.
pub fn estimate_message_tokens(msg: &Message) -> u32 {
    let sum: u32 = msg
        .content
        .iter()
        .map(|block| {
            let char_count = match block {
                MessageContent::Text { text } => text.chars().count(),
                MessageContent::ToolUse { name, input, .. } => {
                    name.chars().count() + input.to_string().chars().count()
                }
                MessageContent::ToolResult { content, .. } => content.chars().count(),
            };
            ((char_count / 4) as u32).max(1)
        })
        .sum();
    sum.max(1)
}

/// Estimate the total number of tokens across a slice of messages.
pub fn estimate_tokens(messages: &[Message]) -> u32 {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Find the index at which to split the conversation for summarisation.
///
/// Keeps `keep_recent` messages intact at the tail. Scans forward from the
/// candidate split point to find the first `User` message (preserving the
/// alternating-turn requirement). Returns `None` if the conversation is too
/// short or no `User` message exists in the scannable region.
pub fn find_summary_split(messages: &[Message], keep_recent: usize) -> Option<usize> {
    if messages.len() <= keep_recent {
        return None;
    }
    let candidate = messages.len() - keep_recent;
    messages[candidate..]
        .iter()
        .position(|msg| msg.role == Role::User)
        .map(|offset| candidate + offset)
}

// ─── Async summarisation ──────────────────────────────────────────────────────

/// Build a summary of `messages` by streaming a completion from `provider`.
///
/// Constructs a single-user-message prompt asking the model to summarise the
/// supplied messages, calls `provider.stream_completion`, collects every
/// `TextDelta` into a `String`, and returns it.
pub async fn summarise_messages(
    messages: &[Message],
    model: &str,
    provider: &dyn Provider,
) -> Result<String> {
    // Build a text representation of the messages to summarise.
    let transcript: String = messages
        .iter()
        .map(|msg| {
            let role_str = match msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
            };
            let text: String = msg
                .content
                .iter()
                .map(|block| match block {
                    MessageContent::Text { text } => text.clone(),
                    MessageContent::ToolUse { name, input, .. } => {
                        format!("[tool: {} {}]", name, input)
                    }
                    MessageContent::ToolResult { content, .. } => content.clone(),
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("{}: {}", role_str, text)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Please summarise the following conversation excerpt concisely, \
         preserving all important context, decisions, and outcomes:\n\n{}",
        transcript
    );

    let summary_messages = vec![Message {
        role: Role::User,
        content: vec![MessageContent::Text { text: prompt }],
    }];

    let mut stream = provider.stream_completion(model, &summary_messages, &[], None);
    let mut summary = String::new();

    use futures::StreamExt;
    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::TextDelta(text)) => summary.push_str(&text),
            Ok(StreamEvent::TurnEnd { .. }) => break,
            Err(e) => return Err(anyhow::anyhow!("summarise_messages stream error: {}", e)),
            _ => {}
        }
    }

    Ok(summary)
}

/// Compress `conv` if its estimated token count exceeds `config.limit * config.threshold`.
///
/// Returns the (possibly compressed) `Conversation` and an optional
/// `TurnEvent::ContextSummarized` event. Returns `Ok((conv, None))` when
/// compression is not needed or not possible.
///
/// **Ownership note:** the caller must clone `conv` before calling this function
/// if it needs a fallback copy for the error path.
pub async fn maybe_compress_context(
    conv: Conversation,
    config: &ContextConfig,
    provider: &dyn Provider,
) -> Result<(Conversation, Option<TurnEvent>)> {
    // No limit configured — compression disabled.
    let Some(limit) = config.limit else {
        return Ok((conv, None));
    };

    let tokens_before = estimate_tokens(&conv.messages);
    let threshold_tokens = (limit as f32 * config.threshold) as u32;

    if tokens_before < threshold_tokens {
        return Ok((conv, None));
    }

    // Find where to split the conversation.
    let Some(split_idx) = find_summary_split(&conv.messages, config.keep_recent) else {
        return Ok((conv, None));
    };

    let messages_before_count = conv.messages.len();

    // Summarise the messages before the split.
    let to_summarise = &conv.messages[..split_idx];
    let summary_text = summarise_messages(to_summarise, &conv.model, provider).await?;

    // Build new message list: summary wrapper + recent tail.
    let summary_msg = Message {
        role: Role::User,
        content: vec![MessageContent::Text {
            text: format!("[Summary of earlier conversation]\n{}", summary_text),
        }],
    };

    let mut new_messages = vec![summary_msg];
    new_messages.extend_from_slice(&conv.messages[split_idx..]);

    let messages_after_count = new_messages.len();
    let tokens_after = estimate_tokens(&new_messages);

    let new_conv = conv.with_messages(new_messages);

    let event = TurnEvent::ContextSummarized {
        messages_before: messages_before_count,
        messages_after: messages_after_count,
        tokens_before,
        tokens_after,
    };

    Ok((new_conv, Some(event)))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text_msg(role: Role, text: &str) -> Message {
        Message {
            role,
            content: vec![MessageContent::Text { text: text.to_string() }],
        }
    }

    fn assistant_msg(text: &str) -> Message {
        text_msg(Role::Assistant, text)
    }

    fn user_msg(text: &str) -> Message {
        text_msg(Role::User, text)
    }

    // ── estimate_message_tokens / estimate_tokens ──────────────────────────

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(&[]), 0);
    }

    #[test]
    fn estimate_tokens_text_message() {
        // "hello world" = 11 chars → 11/4 = 2
        let msg = user_msg("hello world");
        assert_eq!(estimate_message_tokens(&msg), 2);
    }

    #[test]
    fn estimate_tokens_tool_use() {
        // name "bash" = 4 chars, input json!("ls") → "\"ls\"" = 4 chars → total 8 chars → 8/4 = 2
        let msg = Message {
            role: Role::Assistant,
            content: vec![MessageContent::ToolUse {
                id: "t1".to_string(),
                name: "bash".to_string(),
                input: json!("ls"),
            }],
        };
        assert_eq!(estimate_message_tokens(&msg), 2);
    }

    #[test]
    fn estimate_tokens_tool_result() {
        // "output\n" = 7 chars → 7/4 = 1
        let msg = Message {
            role: Role::User,
            content: vec![MessageContent::ToolResult {
                tool_use_id: "t1".to_string(),
                content: "output\n".to_string(),
                is_error: false,
            }],
        };
        assert_eq!(estimate_message_tokens(&msg), 1);
    }

    // ── find_summary_split ─────────────────────────────────────────────────

    #[test]
    fn find_summary_split_too_short() {
        let msgs: Vec<Message> = (0..3).map(|_| user_msg("x")).collect();
        assert_eq!(find_summary_split(&msgs, 5), None);
    }

    #[test]
    fn find_summary_split_finds_user() {
        // 10 messages, index 5 is User, keep_recent=5 → candidate=5, msgs[5] is User → Some(5)
        let mut msgs: Vec<Message> = (0..5).map(|_| assistant_msg("x")).collect();
        msgs.push(user_msg("u")); // index 5 — User
        msgs.extend((0..4).map(|_| assistant_msg("x")));
        assert_eq!(msgs.len(), 10);
        assert_eq!(find_summary_split(&msgs, 5), Some(5));
    }

    #[test]
    fn find_summary_split_skips_to_user() {
        // 10 messages, index 5 is Assistant, index 6 is User, keep_recent=5 → Some(6)
        let mut msgs: Vec<Message> = (0..5).map(|_| user_msg("u")).collect();
        msgs.push(assistant_msg("a")); // index 5 — Assistant
        msgs.push(user_msg("u"));      // index 6 — User
        msgs.extend((0..3).map(|_| assistant_msg("x")));
        assert_eq!(msgs.len(), 10);
        assert_eq!(find_summary_split(&msgs, 5), Some(6));
    }

    #[test]
    fn find_summary_split_no_user_in_tail() {
        // 10 messages, indices 5-9 all Assistant → None
        let mut msgs: Vec<Message> = (0..5).map(|_| user_msg("u")).collect();
        msgs.extend((0..5).map(|_| assistant_msg("a")));
        assert_eq!(msgs.len(), 10);
        assert_eq!(find_summary_split(&msgs, 5), None);
    }

    // ── MockProvider / ErrorProvider ──────────────────────────────────────

    use crate::provider::{ProviderError};
    use crate::types::TurnEvent;
    use futures::stream::{self, BoxStream};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct MockProvider {
        scripts: Arc<Mutex<VecDeque<Vec<StreamEvent>>>>,
    }

    impl MockProvider {
        fn new(scripts: Vec<Vec<StreamEvent>>) -> Self {
            Self {
                scripts: Arc::new(Mutex::new(scripts.into_iter().collect())),
            }
        }
    }

    impl Provider for MockProvider {
        fn stream_completion<'a>(
            &'a self,
            _model: &'a str,
            _messages: &'a [Message],
            _tools: &'a [serde_json::Value],
            _system_prompt: Option<&'a str>,
        ) -> BoxStream<'a, Result<StreamEvent, ProviderError>> {
            let events = self
                .scripts
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default();
            Box::pin(stream::iter(events.into_iter().map(Ok)))
        }
    }

    struct ErrorProvider;

    impl Provider for ErrorProvider {
        fn stream_completion<'a>(
            &'a self,
            _model: &'a str,
            _messages: &'a [Message],
            _tools: &'a [serde_json::Value],
            _system_prompt: Option<&'a str>,
        ) -> BoxStream<'a, Result<StreamEvent, ProviderError>> {
            Box::pin(stream::iter(vec![Err(ProviderError::Aws(
                "network failure".to_string(),
            ))]))
        }
    }

    fn make_conv_with_messages(messages: Vec<Message>) -> Conversation {
        use crate::config::AppConfig;
        Conversation::new("test-id", "claude-3", AppConfig::default()).with_messages(messages)
    }

    // ── summarise_messages ────────────────────────────────────────────────

    #[tokio::test]
    async fn summarise_messages_collects_stream() {
        let provider = MockProvider::new(vec![vec![
            StreamEvent::TextDelta("foo".to_string()),
            StreamEvent::TextDelta("bar".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 5,
                output_tokens: 2,
            },
        ]]);
        let messages = vec![user_msg("hello")];
        let result = summarise_messages(&messages, "test-model", &provider).await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(result.unwrap(), "foobar");
    }

    #[tokio::test]
    async fn summarise_messages_provider_error_returns_err() {
        let provider = ErrorProvider;
        let messages = vec![user_msg("hello")];
        let result = summarise_messages(&messages, "test-model", &provider).await;
        assert!(result.is_err(), "expected Err but got Ok");
    }

    // ── maybe_compress_context ────────────────────────────────────────────

    #[tokio::test]
    async fn maybe_compress_context_no_op_under_threshold() {
        // Very high limit so tokens are well below threshold
        let config = ContextConfig {
            limit: Some(100_000),
            keep_recent: 4,
            threshold: 0.8,
        };
        let messages: Vec<Message> = (0..6)
            .map(|i| if i % 2 == 0 { user_msg("hi") } else { assistant_msg("hello") })
            .collect();
        let conv = make_conv_with_messages(messages.clone());
        let provider = MockProvider::new(vec![]);
        let result = maybe_compress_context(conv, &config, &provider).await;
        assert!(result.is_ok());
        let (returned_conv, event) = result.unwrap();
        assert_eq!(returned_conv.messages.len(), messages.len());
        assert!(event.is_none());
    }

    #[tokio::test]
    async fn maybe_compress_context_compresses_when_over_threshold() {
        // Low limit so tokens are over threshold
        let config = ContextConfig {
            limit: Some(1),
            keep_recent: 4,
            threshold: 0.8,
        };
        // 8 messages: alternating user/assistant
        let messages: Vec<Message> = (0..8)
            .map(|i| {
                if i % 2 == 0 {
                    user_msg("hello world this is a longer message to push tokens over")
                } else {
                    assistant_msg("response with some content here too")
                }
            })
            .collect();
        let original_count = messages.len();
        let conv = make_conv_with_messages(messages);
        let provider = MockProvider::new(vec![vec![
            StreamEvent::TextDelta("summary text".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 5,
                output_tokens: 3,
            },
        ]]);
        let result = maybe_compress_context(conv, &config, &provider).await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let (returned_conv, event) = result.unwrap();
        assert!(
            returned_conv.messages.len() < original_count,
            "expected fewer messages after compression"
        );
        assert!(event.is_some(), "expected ContextSummarized event");
        assert!(matches!(event.unwrap(), TurnEvent::ContextSummarized { .. }));
    }

    #[tokio::test]
    async fn maybe_compress_context_new_messages_start_with_user() {
        let config = ContextConfig {
            limit: Some(1),
            keep_recent: 4,
            threshold: 0.8,
        };
        let messages: Vec<Message> = (0..8)
            .map(|i| {
                if i % 2 == 0 {
                    user_msg("hello world longer message text here for tokens")
                } else {
                    assistant_msg("response assistant text for tokens")
                }
            })
            .collect();
        let conv = make_conv_with_messages(messages);
        let provider = MockProvider::new(vec![vec![
            StreamEvent::TextDelta("summarised".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 5,
                output_tokens: 3,
            },
        ]]);
        let (returned_conv, _event) = maybe_compress_context(conv, &config, &provider)
            .await
            .unwrap();
        assert_eq!(
            returned_conv.messages[0].role,
            Role::User,
            "first message after compression must be User"
        );
    }

    #[tokio::test]
    async fn maybe_compress_context_cannot_split_returns_unchanged() {
        // keep_recent = 20 but only 6 messages → find_summary_split returns None
        let config = ContextConfig {
            limit: Some(1),
            keep_recent: 20,
            threshold: 0.8,
        };
        let messages: Vec<Message> = (0..6)
            .map(|i| if i % 2 == 0 { user_msg("hi") } else { assistant_msg("hello") })
            .collect();
        let conv = make_conv_with_messages(messages.clone());
        let provider = MockProvider::new(vec![]);
        let result = maybe_compress_context(conv, &config, &provider).await;
        assert!(result.is_ok());
        let (returned_conv, event) = result.unwrap();
        assert_eq!(returned_conv.messages.len(), messages.len());
        assert!(event.is_none());
    }
}
