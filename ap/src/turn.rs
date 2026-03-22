// src/turn.rs — Pure async `turn()` pipeline.
//
// Takes an immutable `Conversation` (with the user message already appended via
// `Conversation::with_user_message()`) and returns a new `Conversation` with the
// full assistant response — including any tool calls and their results —
// appended to the message history.
//
// The caller drives the loop:
//   ```rust
//   let conv = turn(conv.with_user_message(input), &provider, &tools, &middleware, &tx).await?;
//   ```

use anyhow::Result;
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::provider::{Message, MessageContent, Provider, Role, StreamEvent};
use crate::tools::{ToolRegistry, ToolResult};
use crate::types::{Conversation, Middleware, ToolCall, ToolMiddlewareResult, TurnEvent};

// ─── Internal accumulator ─────────────────────────────────────────────────────

struct PendingTool {
    id: String,
    name: String,
    params_json: String,
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Execute one complete agent turn as a pure data pipeline.
///
/// * `conv`       — conversation with the user message already appended
/// * `provider`   — LLM streaming provider
/// * `tools`      — registered tools available for execution
/// * `middleware` — pre/post hooks applied at turn and tool boundaries
/// * `tx`         — channel for streaming `TurnEvent`s to the caller
///
/// Returns the updated `Conversation` (original consumed).
pub async fn turn(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
    tx: &mpsc::Sender<TurnEvent>,
) -> Result<Conversation> {
    let conv = apply_pre_turn(conv, middleware);
    turn_loop(conv, provider, tools, middleware, tx).await
}

// ─── Private pipeline steps ───────────────────────────────────────────────────

/// Apply every `pre_turn` middleware in order. Returns the (possibly modified)
/// `Conversation`. A middleware returning `None` is a no-op.
fn apply_pre_turn(mut conv: Conversation, middleware: &Middleware) -> Conversation {
    for f in &middleware.pre_turn {
        if let Some(new_conv) = f(&conv) {
            conv = new_conv;
        }
    }
    conv
}

/// Apply every `post_turn` middleware in order. Returns the (possibly modified)
/// `Conversation`. A middleware returning `None` is a no-op.
fn apply_post_turn(mut conv: Conversation, middleware: &Middleware) -> Conversation {
    for f in &middleware.post_turn {
        if let Some(new_conv) = f(&conv) {
            conv = new_conv;
        }
    }
    conv
}

/// Inner loop — streams the provider, handles tool calls, loops until the LLM
/// sends no more tool calls.
async fn turn_loop(
    mut conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
    tx: &mpsc::Sender<TurnEvent>,
) -> Result<Conversation> {
    loop {
        let tool_schemas = tools.all_schemas();
        // Clone the message snapshot so `stream` doesn't borrow `conv` for the
        // entire loop body (we need to push to `conv.messages` later).
        let messages_snapshot = conv.messages.clone();
        let mut stream = provider.stream_completion(&messages_snapshot, &tool_schemas);

        let mut assistant_text = String::new();
        let mut pending_tools: Vec<PendingTool> = Vec::new();
        let mut current: Option<PendingTool> = None;

        // ── Stream the provider response ──────────────────────────────────────
        while let Some(event) = stream.next().await {
            let event = match event {
                Ok(e) => e,
                Err(e) => {
                    let msg = e.to_string();
                    let _ = tx.send(TurnEvent::Error(msg.clone())).await;
                    return Err(anyhow::anyhow!(msg));
                }
            };

            match event {
                StreamEvent::TextDelta(text) => {
                    let _ = tx.send(TurnEvent::TextChunk(text.clone())).await;
                    assistant_text.push_str(&text);
                }

                StreamEvent::ToolUseStart { id, name } => {
                    current = Some(PendingTool {
                        id,
                        name,
                        params_json: String::new(),
                    });
                }

                StreamEvent::ToolUseParams(fragment) => {
                    if let Some(ref mut tool) = current {
                        tool.params_json.push_str(&fragment);
                    }
                }

                StreamEvent::ToolUseEnd => {
                    if let Some(tool) = current.take() {
                        pending_tools.push(tool);
                    }
                }

                StreamEvent::TurnEnd { .. } => {
                    conv = apply_post_turn(conv, middleware);
                    break;
                }
            }
        }

        // ── Build and append the assistant message ────────────────────────────
        let mut assistant_content: Vec<MessageContent> = Vec::new();
        if !assistant_text.is_empty() {
            assistant_content.push(MessageContent::Text {
                text: assistant_text,
            });
        }
        for tool in &pending_tools {
            let input: serde_json::Value =
                serde_json::from_str(&tool.params_json).unwrap_or(serde_json::Value::Null);
            assistant_content.push(MessageContent::ToolUse {
                id: tool.id.clone(),
                name: tool.name.clone(),
                input,
            });
        }
        if !assistant_content.is_empty() {
            conv.messages.push(Message {
                role: Role::Assistant,
                content: assistant_content,
            });
        }

        // ── No tool calls → turn is complete ─────────────────────────────────
        if pending_tools.is_empty() {
            let _ = tx.send(TurnEvent::TurnEnd).await;
            return Ok(conv);
        }

        // ── Execute each tool call through the middleware chain ───────────────
        let mut tool_results: Vec<MessageContent> = Vec::new();

        for pending in pending_tools {
            let params: serde_json::Value =
                serde_json::from_str(&pending.params_json).unwrap_or(serde_json::Value::Null);

            let call = ToolCall {
                id: pending.id.clone(),
                name: pending.name.clone(),
                params: params.clone(),
            };

            // Emit ToolStart before any middleware so the UI can show progress
            let _ = tx
                .send(TurnEvent::ToolStart {
                    name: call.name.clone(),
                    params: call.params.clone(),
                })
                .await;

            // ── Pre-tool middleware ───────────────────────────────────────────
            let (call, pre_result) = run_pre_tool_chain(call, middleware);

            // ── Execute (or use middleware-supplied result) ───────────────────
            let mut exec_result = if let Some(result) = pre_result {
                result
            } else {
                match tools.find_by_name(&call.name) {
                    Some(t) => t.execute(call.params.clone()).await,
                    None => ToolResult::err(format!("tool not found: {}", call.name)),
                }
            };

            // ── Post-tool middleware ──────────────────────────────────────────
            exec_result = run_post_tool_chain(call.clone(), exec_result, middleware);

            // Emit ToolComplete with the final result string
            let _ = tx
                .send(TurnEvent::ToolComplete {
                    name: call.name.clone(),
                    result: exec_result.content.clone(),
                })
                .await;

            tool_results.push(MessageContent::ToolResult {
                tool_use_id: pending.id,
                content: exec_result.content,
                is_error: exec_result.is_error,
            });
        }

        // ── Append all tool results as a single user turn ─────────────────────
        conv.messages.push(Message {
            role: Role::User,
            content: tool_results,
        });

        // ── Loop back: call LLM with results appended ─────────────────────────
    }
}

/// Fold the pre_tool middleware chain.
///
/// Returns `(ToolCall, Option<ToolResult>)`:
/// - `Some(result)` → skip execution, use this result (Block or Transform)
/// - `None`         → execute the tool using the (possibly modified) `ToolCall`
fn run_pre_tool_chain(
    call: ToolCall,
    middleware: &Middleware,
) -> (ToolCall, Option<ToolResult>) {
    let mut current = call;
    for f in &middleware.pre_tool {
        match f(current.clone()) {
            ToolMiddlewareResult::Allow(c) => {
                current = c;
            }
            ToolMiddlewareResult::Block(reason) => {
                return (
                    current,
                    Some(ToolResult::err(format!("blocked by middleware: {reason}"))),
                );
            }
            ToolMiddlewareResult::Transform(result) => {
                return (current, Some(result));
            }
        }
    }
    (current, None)
}

/// Fold the post_tool middleware chain over the execution result.
///
/// A Transform or Block in post_tool overrides the execution result.
fn run_post_tool_chain(call: ToolCall, result: ToolResult, middleware: &Middleware) -> ToolResult {
    let mut current_result = result;
    for f in &middleware.post_tool {
        match f(call.clone()) {
            ToolMiddlewareResult::Allow(_) => {
                // Keep result as-is
            }
            ToolMiddlewareResult::Block(reason) => {
                current_result = ToolResult::err(format!("post_tool blocked: {reason}"));
                break;
            }
            ToolMiddlewareResult::Transform(new_result) => {
                current_result = new_result;
                break;
            }
        }
    }
    current_result
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::provider::{ProviderError, StreamEvent};
    use crate::types::{Middleware, ToolMiddlewareResult};
    use crate::tools::ToolRegistry;

    use futures::stream::{self, BoxStream};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_conv() -> Conversation {
        Conversation::new("test-id", "claude-3", AppConfig::default())
            .with_user_message("hi")
    }

    fn turn_end_event() -> StreamEvent {
        StreamEvent::TurnEnd {
            stop_reason: "end_turn".to_string(),
            input_tokens: 1,
            output_tokens: 1,
        }
    }

    /// Collect all events from the channel (non-blocking drain).
    async fn drain(rx: &mut mpsc::Receiver<TurnEvent>) -> Vec<TurnEvent> {
        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        events
    }

    // ── MockProvider ──────────────────────────────────────────────────────────

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
            _messages: &'a [crate::provider::Message],
            _tools: &'a [serde_json::Value],
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

    /// A provider that always returns an error on first call.
    struct ErrorProvider;

    impl Provider for ErrorProvider {
        fn stream_completion<'a>(
            &'a self,
            _messages: &'a [crate::provider::Message],
            _tools: &'a [serde_json::Value],
        ) -> BoxStream<'a, Result<StreamEvent, ProviderError>> {
            Box::pin(stream::iter(vec![Err(ProviderError::Aws(
                "network failure".to_string(),
            ))]))
        }
    }

    // ── AC1: Text-only response ───────────────────────────────────────────────

    #[tokio::test]
    async fn turn_text_only_response() {
        let provider = MockProvider::new(vec![vec![
            StreamEvent::TextDelta("Hello".to_string()),
            turn_end_event(),
        ]]);

        let (tx, mut rx) = mpsc::channel(16);
        let tools = ToolRegistry::new();
        let middleware = Middleware::default();

        let result_conv = turn(make_conv(), &provider, &tools, &middleware, &tx)
            .await
            .expect("turn should succeed");

        let events = drain(&mut rx).await;

        // TextChunk("Hello") and TurnEnd emitted
        assert!(
            events
                .iter()
                .any(|e| matches!(e, TurnEvent::TextChunk(s) if s == "Hello")),
            "expected TextChunk(Hello)"
        );
        assert!(
            events.iter().any(|e| matches!(e, TurnEvent::TurnEnd)),
            "expected TurnEnd"
        );

        // Conversation has user + assistant messages
        assert_eq!(
            result_conv.messages.len(),
            2,
            "should have user + assistant messages"
        );
        assert!(matches!(result_conv.messages[0].role, Role::User));
        assert!(matches!(result_conv.messages[1].role, Role::Assistant));
    }

    // ── AC2: TextChunk events arrive in order ─────────────────────────────────

    #[tokio::test]
    async fn turn_emits_text_chunks_in_order() {
        let provider = MockProvider::new(vec![vec![
            StreamEvent::TextDelta("foo".to_string()),
            StreamEvent::TextDelta("bar".to_string()),
            turn_end_event(),
        ]]);

        let (tx, mut rx) = mpsc::channel(16);
        let tools = ToolRegistry::new();
        let middleware = Middleware::default();

        turn(make_conv(), &provider, &tools, &middleware, &tx)
            .await
            .expect("turn should succeed");

        let events = drain(&mut rx).await;

        let chunks: Vec<&str> = events
            .iter()
            .filter_map(|e| {
                if let TurnEvent::TextChunk(s) = e {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(chunks, vec!["foo", "bar"], "chunks must arrive in order");

        let last = events.last().unwrap();
        assert!(matches!(last, TurnEvent::TurnEnd), "last event must be TurnEnd");
    }

    // ── AC3: Tool call triggers execution and second LLM round ───────────────

    #[tokio::test]
    async fn turn_with_tool_call_executes_and_loops() {
        use serde_json::json;

        // First call: tool_use block for "bash"
        // Second call: text response after seeing tool results
        let provider = MockProvider::new(vec![
            vec![
                StreamEvent::ToolUseStart {
                    id: "tool-1".to_string(),
                    name: "bash".to_string(),
                },
                StreamEvent::ToolUseParams(r#"{"command":"echo hi"}"#.to_string()),
                StreamEvent::ToolUseEnd,
                turn_end_event(),
            ],
            vec![
                StreamEvent::TextDelta("done".to_string()),
                turn_end_event(),
            ],
        ]);

        let (tx, mut rx) = mpsc::channel(32);
        let tools = ToolRegistry::with_defaults();
        let middleware = Middleware::default();

        let result_conv = turn(make_conv(), &provider, &tools, &middleware, &tx)
            .await
            .expect("turn should succeed");

        let events = drain(&mut rx).await;

        // ToolStart and ToolComplete events emitted
        assert!(
            events.iter().any(|e| matches!(e, TurnEvent::ToolStart { name, .. } if name == "bash")),
            "expected ToolStart for bash"
        );
        assert!(
            events.iter().any(|e| matches!(e, TurnEvent::ToolComplete { name, .. } if name == "bash")),
            "expected ToolComplete for bash"
        );
        assert!(
            events.iter().any(|e| matches!(e, TurnEvent::TurnEnd)),
            "expected TurnEnd"
        );

        // Conversation: user + assistant(tool_use) + user(tool_result) + assistant(text)
        assert!(
            result_conv.messages.len() >= 3,
            "expected at least 3 messages (user, assistant, tool_result, ...)"
        );

        let _ = json!({}); // suppress unused warning
    }

    // ── AC4: Provider error emits Error event and returns Err ─────────────────

    #[tokio::test]
    async fn turn_provider_error_emits_error_and_returns_err() {
        let provider = ErrorProvider;

        let (tx, mut rx) = mpsc::channel(16);
        let tools = ToolRegistry::new();
        let middleware = Middleware::default();

        let result = turn(make_conv(), &provider, &tools, &middleware, &tx).await;

        assert!(result.is_err(), "turn should return Err on provider error");

        let events = drain(&mut rx).await;
        let has_error = events.iter().any(|e| {
            if let TurnEvent::Error(msg) = e {
                msg.contains("network failure")
            } else {
                false
            }
        });
        assert!(has_error, "expected TurnEvent::Error with 'network failure'");
    }

    // ── AC5: Pre-tool Block middleware skips execution ────────────────────────

    #[tokio::test]
    async fn turn_pre_tool_block_skips_execution() {
        let provider = MockProvider::new(vec![
            // First LLM call returns a tool_use
            vec![
                StreamEvent::ToolUseStart {
                    id: "t1".to_string(),
                    name: "bash".to_string(),
                },
                StreamEvent::ToolUseParams(r#"{"command":"rm -rf /"}"#.to_string()),
                StreamEvent::ToolUseEnd,
                turn_end_event(),
            ],
            // Second LLM call (with blocked result) returns text
            vec![
                StreamEvent::TextDelta("okay".to_string()),
                turn_end_event(),
            ],
        ]);

        let (tx, mut rx) = mpsc::channel(32);
        let tools = ToolRegistry::with_defaults();

        // Middleware that blocks all tool calls
        let mut middleware = Middleware::default();
        middleware.pre_tool.push(Box::new(|call| {
            ToolMiddlewareResult::Block(format!("not allowed: {}", call.name))
        }));

        turn(make_conv(), &provider, &tools, &middleware, &tx)
            .await
            .expect("turn should succeed even when tool is blocked");

        let events = drain(&mut rx).await;

        // ToolComplete event should carry the block reason
        let complete_evt = events.iter().find(|e| {
            matches!(e, TurnEvent::ToolComplete { name, .. } if name == "bash")
        });
        assert!(complete_evt.is_some(), "expected ToolComplete for bash");

        if let Some(TurnEvent::ToolComplete { result, .. }) = complete_evt {
            assert!(
                result.contains("blocked by middleware"),
                "result should mention 'blocked by middleware', got: {result}"
            );
        }
    }

    // ── AC6: Pre-tool Transform middleware skips execution ────────────────────

    #[tokio::test]
    async fn turn_pre_tool_transform_skips_execution() {
        let provider = MockProvider::new(vec![
            vec![
                StreamEvent::ToolUseStart {
                    id: "t1".to_string(),
                    name: "bash".to_string(),
                },
                StreamEvent::ToolUseParams(r#"{"command":"slow command"}"#.to_string()),
                StreamEvent::ToolUseEnd,
                turn_end_event(),
            ],
            vec![
                StreamEvent::TextDelta("got mock".to_string()),
                turn_end_event(),
            ],
        ]);

        let (tx, mut rx) = mpsc::channel(32);
        let tools = ToolRegistry::with_defaults();

        let mut middleware = Middleware::default();
        middleware.pre_tool.push(Box::new(|_call| {
            ToolMiddlewareResult::Transform(ToolResult::ok("mocked result"))
        }));

        turn(make_conv(), &provider, &tools, &middleware, &tx)
            .await
            .expect("turn should succeed");

        let events = drain(&mut rx).await;

        // ToolComplete carries the mocked result
        let complete_evt = events.iter().find(|e| {
            matches!(e, TurnEvent::ToolComplete { name, .. } if name == "bash")
        });
        assert!(complete_evt.is_some(), "expected ToolComplete for bash");

        if let Some(TurnEvent::ToolComplete { result, .. }) = complete_evt {
            assert_eq!(result, "mocked result", "expected mocked result");
        }
    }

    // ── AC7: Pre-tool Allow passes through (possibly modified) ────────────────

    #[tokio::test]
    async fn turn_pre_tool_allow_passes_through() {
        let provider = MockProvider::new(vec![
            vec![
                StreamEvent::ToolUseStart {
                    id: "t1".to_string(),
                    name: "bash".to_string(),
                },
                StreamEvent::ToolUseParams(r#"{"command":"echo hello"}"#.to_string()),
                StreamEvent::ToolUseEnd,
                turn_end_event(),
            ],
            vec![
                StreamEvent::TextDelta("done".to_string()),
                turn_end_event(),
            ],
        ]);

        let (tx, mut rx) = mpsc::channel(32);
        let tools = ToolRegistry::with_defaults();

        let mut middleware = Middleware::default();
        // Allow but modify the call (swap to echo something else)
        middleware.pre_tool.push(Box::new(|mut call| {
            call.params = serde_json::json!({"command": "echo modified"});
            ToolMiddlewareResult::Allow(call)
        }));

        turn(make_conv(), &provider, &tools, &middleware, &tx)
            .await
            .expect("turn should succeed");

        let events = drain(&mut rx).await;

        // Tool was actually executed (ToolComplete present)
        assert!(
            events
                .iter()
                .any(|e| matches!(e, TurnEvent::ToolComplete { name, .. } if name == "bash")),
            "expected ToolComplete for bash"
        );
        // TurnEnd emitted
        assert!(
            events.iter().any(|e| matches!(e, TurnEvent::TurnEnd)),
            "expected TurnEnd"
        );
    }
}
