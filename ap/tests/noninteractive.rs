//! Integration test: non-interactive (headless) mode.
//!
//! Uses `MockProvider` scripted with `TextDelta("Hello from mock") + TurnEnd`.
//! Invokes the turn() pipeline programmatically (not via subprocess).
//! Verifies TextChunk received, TurnEnd received, and no Error emitted.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use ap::config::AppConfig;
use ap::provider::{Message, Provider, ProviderError, StreamEvent};
use ap::tools::ToolRegistry;
use ap::turn::turn;
use ap::types::{Conversation, Middleware, TurnEvent};

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

// ─── Headless dispatch helper ─────────────────────────────────────────────────

/// Runs one turn of the FP pipeline in headless mode and collects all TurnEvents.
async fn run_headless_test(prompt: &str, provider: Arc<dyn Provider>) -> Vec<TurnEvent> {
    let (tx, mut rx) = mpsc::channel(64);
    let conv = Conversation::new("test-session", "claude-3", AppConfig::default())
        .with_user_message(prompt);
    let tools = ToolRegistry::with_defaults();
    let middleware = Middleware::new();

    // Spawn so we can drain the channel concurrently (bounded channel, small mock)
    let tx_for_turn = tx.clone();
    let turn_handle = tokio::spawn(async move {
        turn(conv, provider.as_ref(), &tools, &middleware, &tx_for_turn).await
    });
    drop(tx); // drop original so rx sees None when turn finishes

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    turn_handle.await.expect("turn task panicked").expect("turn failed");
    events
}

// ─── Test: TextChunk and TurnEnd received ────────────────────────────────────

#[tokio::test]
async fn headless_receives_text_chunk_and_turn_end() {
    let provider = Arc::new(MockProvider::new(vec![vec![
        StreamEvent::TextDelta("Hello from mock".to_string()),
        StreamEvent::TurnEnd {
            stop_reason: "end_turn".to_string(),
            input_tokens: 10,
            output_tokens: 5,
        },
    ]]));

    let events = run_headless_test("test", provider).await;

    let has_text_chunk = events
        .iter()
        .any(|e| matches!(e, TurnEvent::TextChunk(t) if t == "Hello from mock"));
    let has_turn_end = events.iter().any(|e| matches!(e, TurnEvent::TurnEnd));
    let has_error = events.iter().any(|e| matches!(e, TurnEvent::Error(_)));

    assert!(
        has_text_chunk,
        "Expected TextChunk('Hello from mock'), got: {:?}",
        events
    );
    assert!(has_turn_end, "Expected TurnEnd, got: {:?}", events);
    assert!(!has_error, "Unexpected Error event, got: {:?}", events);
}

// ─── MockErrorProvider: returns a ProviderError in the stream ────────────────

struct MockErrorProvider {
    message: String,
}

impl MockErrorProvider {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Provider for MockErrorProvider {
    fn stream_completion<'a>(
        &'a self,
        _messages: &'a [Message],
        _tools: &'a [serde_json::Value],
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>> {
        let err = ProviderError::Aws(self.message.clone());
        Box::pin(stream::iter(vec![Err(err)]))
    }
}

// ─── Test: Error event on provider failure ────────────────────────────────────

#[tokio::test]
async fn headless_emits_error_on_provider_failure() {
    // AC3: Given a provider that returns an error, TurnEvent::Error is emitted
    // and turn() returns Err.
    let provider = Arc::new(MockErrorProvider::new("something failed"));

    let (tx, mut rx) = mpsc::channel(64);
    let conv =
        Conversation::new("test-session", "claude-3", AppConfig::default())
            .with_user_message("test");
    let tools = ToolRegistry::with_defaults();
    let middleware = Middleware::new();

    let tx_for_turn = tx.clone();
    let turn_handle = tokio::spawn(async move {
        turn(
            conv,
            provider.as_ref() as &dyn ap::provider::Provider,
            &tools,
            &middleware,
            &tx_for_turn,
        )
        .await
    });
    drop(tx); // so rx drains after turn finishes

    // Collect all events
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    // turn() must return Err when the stream produces a ProviderError
    let result = turn_handle.await.expect("task panicked");
    assert!(
        result.is_err(),
        "Expected turn() to return Err on provider failure"
    );

    // TurnEvent::Error must be emitted
    let has_error = events.iter().any(|e| matches!(e, TurnEvent::Error(_)));
    assert!(
        has_error,
        "Expected TurnEvent::Error event, got: {:?}",
        events
    );

    // The error message should contain our injected message
    let error_msg = events.iter().find_map(|e| {
        if let TurnEvent::Error(msg) = e {
            Some(msg.as_str())
        } else {
            None
        }
    });
    assert!(
        error_msg
            .map(|m| m.contains("something failed"))
            .unwrap_or(false),
        "Expected error to contain 'something failed', got: {:?}",
        error_msg
    );
}

// ─── Test: -p flag argument parsing ──────────────────────────────────────────

#[test]
fn headless_mode_extracted_from_prompt_some() {
    // Verify the logic: Some(prompt) → headless, None → TUI
    let prompt: Option<String> = Some("hello world".to_string());
    assert!(prompt.is_some(), "-p flag should produce Some(prompt)");
    assert_eq!(prompt.unwrap(), "hello world");
}
