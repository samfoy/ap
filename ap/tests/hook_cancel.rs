//! Integration test: pre_tool_call hook cancels bash tool execution.
//!
//! MockProvider is scripted with:
//!   1. ToolUseStart("bash") → ToolUseParams → ToolUseEnd → TurnEnd(tool_use)
//!   2. TextDelta("Cancelled.") → TurnEnd(end_turn)
//!
//! Verifies: bash NOT executed, synthetic error ToolResult in messages,
//! ToolComplete with is_error=true received on UiEvent channel.

use std::collections::VecDeque;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use ap::app::{AgentLoop, UiEvent};
use ap::config::HooksConfig;
use ap::hooks::HookRunner;
use ap::provider::{Message, MessageContent, Provider, ProviderError, StreamEvent};
use ap::tools::ToolRegistry;

use futures::stream::{self, BoxStream};
use tempfile::NamedTempFile;
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

// ─── Helper: create executable shell script ──────────────────────────────────

fn make_script(body: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    writeln!(f, "{body}").unwrap();
    let mut perms = f.as_file().metadata().unwrap().permissions();
    perms.set_mode(0o755);
    f.as_file().set_permissions(perms).unwrap();
    f
}

// ─── Test: hook cancel prevents tool execution ───────────────────────────────

#[tokio::test]
async fn hook_cancel_prevents_tool_execution() {
    // Hook that always exits 1 (cancels any tool call)
    let cancel_hook = make_script("echo 'blocked by test policy' >&2; exit 1");
    let hooks_config = HooksConfig {
        pre_tool_call: Some(cancel_hook.path().to_str().unwrap().to_string()),
        ..Default::default()
    };

    let provider = Arc::new(MockProvider::new(vec![
        vec![
            StreamEvent::ToolUseStart {
                id: "tool_1".to_string(),
                name: "bash".to_string(),
            },
            StreamEvent::ToolUseParams(r#"{"cmd":"echo dangerous_marker_12345"}"#.to_string()),
            StreamEvent::ToolUseEnd,
            StreamEvent::TurnEnd {
                stop_reason: "tool_use".to_string(),
                input_tokens: 10,
                output_tokens: 5,
            },
        ],
        vec![
            StreamEvent::TextDelta("Cancelled.".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 20,
                output_tokens: 5,
            },
        ],
    ]));

    let (tx, mut rx) = mpsc::channel(64);
    let mut agent = AgentLoop::new(
        provider,
        ToolRegistry::with_defaults(),
        HookRunner::new(hooks_config),
        tx,
    );

    agent
        .run_turn("do something dangerous".into())
        .await
        .expect("run_turn failed");

    // messages: user, assistant(tool_use), user(tool_result), assistant(text) = 4
    assert_eq!(
        agent.messages.len(),
        4,
        "expected 4 messages, got {}",
        agent.messages.len()
    );

    // The tool result message (index 2) must have is_error = true
    let tool_result_msg = &agent.messages[2];
    let has_error_result = tool_result_msg.content.iter().any(|c| match c {
        MessageContent::ToolResult { is_error, .. } => *is_error,
        _ => false,
    });
    assert!(
        has_error_result,
        "expected tool result with is_error=true in messages[2], got: {:?}",
        tool_result_msg.content
    );

    // ToolComplete with is_error=true must appear on UiEvent channel
    let mut got_cancelled_tool_complete = false;
    while let Ok(event) = rx.try_recv() {
        if let UiEvent::ToolComplete { name, result } = event {
            if name == "bash" && result.is_error {
                got_cancelled_tool_complete = true;
            }
        }
    }
    assert!(
        got_cancelled_tool_complete,
        "expected ToolComplete(bash, is_error=true) from hook cancellation"
    );
}

// ─── Test: cancelled tool still results in follow-up LLM turn ────────────────

#[tokio::test]
async fn cancelled_tool_still_loops_to_llm() {
    let cancel_hook = make_script("exit 1");
    let hooks_config = HooksConfig {
        pre_tool_call: Some(cancel_hook.path().to_str().unwrap().to_string()),
        ..Default::default()
    };

    let provider = Arc::new(MockProvider::new(vec![
        vec![
            StreamEvent::ToolUseStart {
                id: "tool_2".to_string(),
                name: "bash".to_string(),
            },
            StreamEvent::ToolUseParams(r#"{"cmd":"ls"}"#.to_string()),
            StreamEvent::ToolUseEnd,
            StreamEvent::TurnEnd {
                stop_reason: "tool_use".to_string(),
                input_tokens: 5,
                output_tokens: 3,
            },
        ],
        vec![
            StreamEvent::TextDelta("ok".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 10,
                output_tokens: 2,
            },
        ],
    ]));

    let (tx, _rx) = mpsc::channel(64);
    let mut agent = AgentLoop::new(
        provider,
        ToolRegistry::with_defaults(),
        HookRunner::new(hooks_config),
        tx,
    );

    // Should complete without error — cancelled tool still feeds back to LLM
    agent
        .run_turn("test cancelled loop".into())
        .await
        .expect("run_turn failed after hook cancel");
}
