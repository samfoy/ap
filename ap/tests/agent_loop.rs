//! Integration test: full agent turn with one tool use (read tool).
//!
//! MockProvider is scripted with two call sequences:
//!   1. ToolUseStart → ToolUseParams → ToolUseEnd → TurnEnd(tool_use)
//!   2. TextDelta("Done.") → TurnEnd(end_turn)
//!
//! Verifies: tool dispatched, messages history correct, TurnEnd UiEvent received.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use ap::app::{AgentLoop, UiEvent};
use ap::config::HooksConfig;
use ap::hooks::HookRunner;
use ap::provider::{Message, Provider, ProviderError, StreamEvent};
use ap::tools::ToolRegistry;

use futures::stream::{self, BoxStream};
use tokio::sync::mpsc;

// ─── MockProvider ─────────────────────────────────────────────────────────────

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
        _messages: &'a [Message],
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

// ─── Test: TextChunk events emitted ──────────────────────────────────────────

#[tokio::test]
async fn text_chunk_events_emitted() {
    let provider = Arc::new(MockProvider::new(vec![
        vec![
            StreamEvent::TextDelta("hello".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 5,
                output_tokens: 5,
            },
        ],
    ]));

    let (tx, mut rx) = mpsc::channel(32);
    let mut agent = AgentLoop::new(
        provider,
        ToolRegistry::with_defaults(),
        HookRunner::new(HooksConfig::default()),
        tx,
    );

    agent.run_turn("hello".into()).await.expect("run_turn failed");

    let mut got_text_chunk = false;
    let mut got_turn_end = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            UiEvent::TextChunk(text) if text == "hello" => got_text_chunk = true,
            UiEvent::TurnEnd => got_turn_end = true,
            _ => {}
        }
    }
    assert!(got_text_chunk, "expected TextChunk(hello)");
    assert!(got_turn_end, "expected TurnEnd");
}

// ─── Test: No tool calls = single LLM call ───────────────────────────────────

#[tokio::test]
async fn no_tool_calls_single_llm_call() {
    // Only one script entry. If provider is called twice, it returns an empty
    // stream which would break the loop — so we rely on correct single-call
    // behaviour.
    let provider = Arc::new(MockProvider::new(vec![vec![
        StreamEvent::TextDelta("only once".to_string()),
        StreamEvent::TurnEnd {
            stop_reason: "end_turn".to_string(),
            input_tokens: 3,
            output_tokens: 3,
        },
    ]]));

    let (tx, _rx) = mpsc::channel(32);
    let mut agent = AgentLoop::new(
        provider,
        ToolRegistry::with_defaults(),
        HookRunner::new(HooksConfig::default()),
        tx,
    );

    agent.run_turn("just text".into()).await.expect("run_turn failed");

    // After the single-call turn: user + assistant = 2 messages
    assert_eq!(agent.messages.len(), 2, "expected 2 messages (user + assistant)");
}

// ─── Test: Full turn with one tool use ───────────────────────────────────────

#[tokio::test]
async fn full_turn_with_tool_use() {
    // Call 1: read tool invocation
    // Call 2: text response
    let provider = Arc::new(MockProvider::new(vec![
        vec![
            StreamEvent::ToolUseStart {
                id: "tool_1".to_string(),
                name: "read".to_string(),
            },
            StreamEvent::ToolUseParams(r#"{"path":"Cargo.toml"}"#.to_string()),
            StreamEvent::ToolUseEnd,
            StreamEvent::TurnEnd {
                stop_reason: "tool_use".to_string(),
                input_tokens: 10,
                output_tokens: 5,
            },
        ],
        vec![
            StreamEvent::TextDelta("Done reading.".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 30,
                output_tokens: 10,
            },
        ],
    ]));

    let (tx, mut rx) = mpsc::channel(64);
    let mut agent = AgentLoop::new(
        provider,
        ToolRegistry::with_defaults(),
        HookRunner::new(HooksConfig::default()),
        tx,
    );

    agent.run_turn("read Cargo.toml".into()).await.expect("run_turn failed");

    // After two turns: user, assistant(tool_use), user(tool_result), assistant(text) = 4
    assert_eq!(
        agent.messages.len(),
        4,
        "expected 4 messages, got {}: {:?}",
        agent.messages.len(),
        agent.messages
    );

    // Check that ToolStart and ToolComplete events were sent
    let mut got_tool_start = false;
    let mut got_tool_complete = false;
    let mut got_turn_end = false;
    while let Ok(event) = rx.try_recv() {
        match &event {
            UiEvent::ToolStart { name, .. } if name == "read" => got_tool_start = true,
            UiEvent::ToolComplete { name, .. } if name == "read" => got_tool_complete = true,
            UiEvent::TurnEnd => got_turn_end = true,
            _ => {}
        }
    }
    assert!(got_tool_start, "expected ToolStart event");
    assert!(got_tool_complete, "expected ToolComplete event");
    assert!(got_turn_end, "expected TurnEnd event");
}
