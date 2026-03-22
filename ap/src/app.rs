//! `app.rs` — AgentLoop: the core orchestration engine.
//!
//! Manages conversation state, streams LLM responses, dispatches tool calls,
//! fires lifecycle hooks, and emits [`UiEvent`]s over a tokio mpsc channel.

use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::hooks::{HookOutcome, HookRunner};
use crate::provider::{Message, MessageContent, Provider, Role, StreamEvent};
use crate::session::{store::SessionStore, Session};
use crate::tools::{ToolRegistry, ToolResult};

// ─── UiEvent ──────────────────────────────────────────────────────────────────

/// Events sent from the agent loop to the TUI (or stdout in `-p` mode).
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// A text chunk streamed from the assistant.
    TextChunk(String),
    /// A tool call is about to execute.
    ToolStart {
        name: String,
        params: serde_json::Value,
    },
    /// A tool call has completed.
    ToolComplete {
        name: String,
        result: ToolResult,
    },
    /// The full agent turn is finished (no more tool calls pending).
    TurnEnd,
    /// An unrecoverable error occurred.
    Error(String),
}

// ─── Pending tool call (internal) ────────────────────────────────────────────

struct PendingTool {
    id: String,
    name: String,
    params_json: String,
}

// ─── AgentLoop ────────────────────────────────────────────────────────────────

/// The agent loop ties together provider, tools, hooks, and the UI channel.
pub struct AgentLoop {
    /// Full conversation history (user + assistant + tool results).
    pub messages: Vec<Message>,
    provider: Arc<dyn Provider>,
    tools: ToolRegistry,
    hooks: HookRunner,
    ui_tx: mpsc::Sender<UiEvent>,
    /// Active session (if persistence is enabled).
    session: Option<Session>,
    /// Store used for session autosave (if persistence is enabled).
    store: Option<SessionStore>,
}

impl AgentLoop {
    /// Construct a new agent loop.
    pub fn new(
        provider: Arc<dyn Provider>,
        tools: ToolRegistry,
        hooks: HookRunner,
        ui_tx: mpsc::Sender<UiEvent>,
    ) -> Self {
        Self {
            messages: Vec::new(),
            provider,
            tools,
            hooks,
            ui_tx,
            session: None,
            store: None,
        }
    }

    /// Construct a new agent loop with an optional session for persistence.
    ///
    /// Uses the default `~/.ap/sessions/` store. If the home directory cannot
    /// be determined, autosave is silently disabled.
    pub fn with_session(
        provider: Arc<dyn Provider>,
        tools: ToolRegistry,
        hooks: HookRunner,
        ui_tx: mpsc::Sender<UiEvent>,
        session: Option<Session>,
    ) -> Self {
        let store = SessionStore::new().ok();
        Self::with_session_store(provider, tools, hooks, ui_tx, session, store)
    }

    /// Construct a new agent loop with an optional session and a custom store.
    ///
    /// Primarily useful in tests where you want autosave directed at a `tempdir`
    /// instead of `~/.ap/sessions/`.
    pub fn with_session_store(
        provider: Arc<dyn Provider>,
        tools: ToolRegistry,
        hooks: HookRunner,
        ui_tx: mpsc::Sender<UiEvent>,
        session: Option<Session>,
        store: impl Into<Option<SessionStore>>,
    ) -> Self {
        let messages = session
            .as_ref()
            .map(|s| s.messages.clone())
            .unwrap_or_default();
        Self {
            messages,
            provider,
            tools,
            hooks,
            ui_tx,
            session,
            store: store.into(),
        }
    }

    /// Send a [`UiEvent`], ignoring send errors (receiver may have closed).
    async fn emit(&self, event: UiEvent) {
        let _ = self.ui_tx.send(event).await;
    }

    /// Persist the current message history into the active session (if any).
    fn autosave_session(&mut self) {
        if let (Some(ref mut session), Some(ref store)) = (&mut self.session, &self.store) {
            session.messages = self.messages.clone();
            if let Err(e) = store.save(session) {
                // Non-fatal: warn but don't crash the agent loop
                eprintln!("ap: warning: failed to save session: {e}");
            }
        }
    }

    /// Execute one complete agent turn, looping until no tool calls remain.
    ///
    /// 1. Appends the user message to history.
    /// 2. Fires `pre_turn` observer hook.
    /// 3. Streams from the provider; accumulates text + tool calls.
    /// 4. After `TurnEnd`: fires `post_turn` hook, executes tools (with hooks).
    /// 5. If tools were executed, appends results and loops back to step 3.
    /// 6. If no tools, emits [`UiEvent::TurnEnd`] and returns.
    pub async fn run_turn(&mut self, user_input: String) -> Result<()> {
        self.messages.push(Message::user(user_input));

        // Pre-turn observer hook
        let messages_json = serde_json::to_string(&self.messages).unwrap_or_default();
        self.hooks.run_observer_hook(
            self.hooks.config.pre_turn.as_deref(),
            vec![("AP_MESSAGES_FILE".to_string(), messages_json)],
        );

        // Main agent loop (may execute multiple LLM turns when tools are used)
        loop {
            // Clone messages and Arc-clone provider so the stream doesn't borrow
            // `self.messages` — letting us push to it after the stream is consumed.
            let messages_snapshot = self.messages.clone();
            let tool_schemas = self.tools.all_schemas();
            let provider = Arc::clone(&self.provider);
            let mut stream = provider.stream_completion(&messages_snapshot, &tool_schemas);

            let mut assistant_text = String::new();
            let mut pending_tools: Vec<PendingTool> = Vec::new();
            let mut current: Option<PendingTool> = None;

            // Consume the streaming response
            while let Some(event) = stream.next().await {
                match event? {
                    StreamEvent::TextDelta(text) => {
                        self.emit(UiEvent::TextChunk(text.clone())).await;
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
                        // Post-turn observer hook
                        let messages_json =
                            serde_json::to_string(&self.messages).unwrap_or_default();
                        self.hooks.run_observer_hook(
                            self.hooks.config.post_turn.as_deref(),
                            vec![("AP_MESSAGES_FILE".to_string(), messages_json)],
                        );
                        break;
                    }
                }
            }

            // Build and append the assistant message (text + tool_use blocks)
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
                self.messages.push(Message {
                    role: Role::Assistant,
                    content: assistant_content,
                });
            }

            // No tool calls → turn is done
            if pending_tools.is_empty() {
                self.emit(UiEvent::TurnEnd).await;
                self.autosave_session();
                return Ok(());
            }

            // Execute each tool call sequentially (R4.1)
            let mut tool_results: Vec<MessageContent> = Vec::new();
            for tool in pending_tools {
                let params: serde_json::Value =
                    serde_json::from_str(&tool.params_json).unwrap_or(serde_json::Value::Null);

                // Pre-tool-call hook
                match self.hooks.run_pre_tool_call(&tool.name, &params) {
                    HookOutcome::Cancelled(reason) => {
                        // R4.3: cancelled → synthetic error, remaining tools still run
                        let result = ToolResult::err(format!("cancelled by hook: {reason}"));
                        self.emit(UiEvent::ToolComplete {
                            name: tool.name.clone(),
                            result: result.clone(),
                        })
                        .await;
                        tool_results.push(MessageContent::ToolResult {
                            tool_use_id: tool.id,
                            content: result.content,
                            is_error: true,
                        });
                        continue;
                    }
                    HookOutcome::HookWarning(warn) => {
                        // Non-fatal: log and continue
                        self.emit(UiEvent::Error(format!("pre_tool_call warning: {warn}")))
                            .await;
                    }
                    _ => {} // Proceed / Observed / Transformed (shouldn't happen for pre)
                }

                // Emit ToolStart
                self.emit(UiEvent::ToolStart {
                    name: tool.name.clone(),
                    params: params.clone(),
                })
                .await;

                // Execute the tool
                let mut result = match self.tools.find_by_name(&tool.name) {
                    Some(t) => t.execute(params.clone()).await,
                    None => ToolResult::err(format!("tool not found: {}", tool.name)),
                };

                // Post-tool-call hook (may transform result)
                match self.hooks.run_post_tool_call(&tool.name, &params, &result) {
                    HookOutcome::Transformed(content) => {
                        result = ToolResult {
                            content,
                            is_error: false,
                        };
                    }
                    HookOutcome::HookWarning(warn) => {
                        self.emit(UiEvent::Error(format!("post_tool_call warning: {warn}")))
                            .await;
                    }
                    _ => {}
                }

                self.emit(UiEvent::ToolComplete {
                    name: tool.name.clone(),
                    result: result.clone(),
                })
                .await;

                tool_results.push(MessageContent::ToolResult {
                    tool_use_id: tool.id,
                    content: result.content,
                    is_error: result.is_error,
                });
            }

            // Append all tool results as a single user turn (R4.4)
            self.messages.push(Message {
                role: Role::User,
                content: tool_results,
            });

            // Loop back to call LLM with the results appended
        }
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HooksConfig;
    use crate::provider::{ProviderError, StreamEvent};
    use crate::session::{store::SessionStore, Session};

    use futures::stream::{self, BoxStream};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    // Minimal mock provider for unit tests
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

    #[tokio::test]
    async fn ui_event_turn_end_sent_on_no_tool_calls() {
        let provider = Arc::new(MockProvider::new(vec![vec![
            StreamEvent::TextDelta("hi".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 1,
                output_tokens: 1,
            },
        ]]));

        let (tx, mut rx) = mpsc::channel(16);
        let mut agent = AgentLoop::new(
            provider,
            ToolRegistry::with_defaults(),
            HookRunner::new(HooksConfig::default()),
            tx,
        );

        agent.run_turn("test".into()).await.unwrap();

        let mut got_turn_end = false;
        while let Ok(event) = rx.try_recv() {
            if matches!(event, UiEvent::TurnEnd) {
                got_turn_end = true;
            }
        }
        assert!(got_turn_end, "expected UiEvent::TurnEnd");
    }

    #[tokio::test]
    async fn messages_grow_correctly_after_no_tool_call_turn() {
        let provider = Arc::new(MockProvider::new(vec![vec![
            StreamEvent::TextDelta("response".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 5,
                output_tokens: 5,
            },
        ]]));

        let (tx, _rx) = mpsc::channel(16);
        let mut agent = AgentLoop::new(
            provider,
            ToolRegistry::with_defaults(),
            HookRunner::new(HooksConfig::default()),
            tx,
        );

        agent.run_turn("input".into()).await.unwrap();

        // user message + assistant message = 2
        assert_eq!(agent.messages.len(), 2);
        assert!(matches!(agent.messages[0].role, Role::User));
        assert!(matches!(agent.messages[1].role, Role::Assistant));
    }

    // ── with_session_seeds_messages_and_autosaves ─────────────────────────────
    // FAIL-2 fix: verify that with_session_store seeds messages from the session
    // and that autosave_session writes the file after run_turn.

    #[tokio::test]
    async fn with_session_seeds_messages_and_autosaves() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());

        // Build a session with one pre-existing message
        let mut session = Session::new("autosave-test".to_string(), "claude".to_string());
        session
            .messages
            .push(crate::provider::Message::user("previous message"));

        let provider = Arc::new(MockProvider::new(vec![vec![
            StreamEvent::TextDelta("hi".to_string()),
            StreamEvent::TurnEnd {
                stop_reason: "end_turn".to_string(),
                input_tokens: 1,
                output_tokens: 1,
            },
        ]]));

        let (tx, _rx) = mpsc::channel(16);
        let mut agent = AgentLoop::with_session_store(
            provider,
            ToolRegistry::with_defaults(),
            HookRunner::new(HooksConfig::default()),
            tx,
            Some(session),
            store,
        );

        // Messages should be seeded from the session before any turn
        assert_eq!(
            agent.messages.len(),
            1,
            "with_session_store should seed messages from session"
        );

        agent.run_turn("new input".into()).await.unwrap();

        // After run_turn, the session file must exist on disk
        let session_file = tmp.path().join("autosave-test.json");
        assert!(
            session_file.exists(),
            "session file should be autosaved after run_turn"
        );

        // The saved session must contain at least: previous msg + user + assistant
        let raw = std::fs::read_to_string(&session_file).unwrap();
        let loaded: crate::session::Session = serde_json::from_str(&raw).unwrap();
        assert!(
            loaded.messages.len() >= 2,
            "saved session should have pre-existing + new messages, got {}",
            loaded.messages.len()
        );
    }
}
