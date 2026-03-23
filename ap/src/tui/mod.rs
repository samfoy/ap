//! `tui/mod.rs` — Ratatui TUI application state and main event loop.
//!
//! [`TuiApp`] holds an immutable [`Conversation`] (behind an `Arc<Mutex>`) plus
//! references to the provider, tools, and middleware.  On submit it spawns a
//! tokio task calling the pure [`turn()`] function, then sends the returned
//! [`TurnEvent`]s through an internal channel to update the UI.
//!
//! The ratatui `Terminal` is kept as a local in `run()` to avoid borrow
//! conflicts between `terminal.draw()` (which needs `&mut Terminal`) and
//! `ui::render()` (which needs `&TuiApp`).

use std::io::{self, Stdout};
use std::sync::Arc;

use anyhow::Result;
use crossterm::{
    event::EventStream,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::{mpsc, oneshot};

use crate::provider::Provider;
use crate::tools::ToolRegistry;
use crate::turn::turn;
use crate::types::{Conversation, Middleware, TurnEvent};
use crate::context::maybe_compress_context;
use crate::config::ContextConfig;

pub mod events;
pub mod theme;
pub mod ui;

pub use theme::Theme;

// ─── ChatBlock / ChatEntry ────────────────────────────────────────────────────

/// A single block inside an assistant message — either prose or a fenced code
/// block.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatBlock {
    /// Plain prose text.
    Text(String),
    /// A fenced code block with an optional language tag.
    Code {
        /// Language hint from the opening fence (e.g. `"rust"`, `""`).
        lang: String,
        /// Raw content of the code block.
        content: String,
    },
}

/// A single entry in the conversation history.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEntry {
    /// Text submitted by the user.
    User(String),
    /// Assistant message that is still streaming — holds the raw accumulated
    /// text so far.
    AssistantStreaming(String),
    /// Assistant message that has finished streaming — parsed into blocks.
    AssistantDone(Vec<ChatBlock>),
    /// A tool call shown inline in the chat history.
    ToolCall {
        /// Tool name (e.g. `"bash"`, `"read"`).
        name: String,
        /// Current execution status.
        status: ToolStatus,
        /// Truncated error output when `status == Error`; `None` for success.
        output_snippet: Option<String>,
    },
}

/// Parse a raw text string into a sequence of [`ChatBlock`]s by scanning for
/// Markdown-style fenced code blocks (triple backticks).
///
/// Rules:
/// - Empty input → empty `Vec`.
/// - No fence → `[Text(full_text)]`.
/// - ` ```lang ` opens a code block; ` ``` ` closes it; remaining text after
///   the close becomes another text/code block as normal.
/// - Unclosed fence → the remaining content is returned as a `Code` block.
pub fn parse_chat_blocks(text: &str) -> Vec<ChatBlock> {
    if text.is_empty() {
        return vec![];
    }

    let mut blocks: Vec<ChatBlock> = Vec::new();
    let mut current_text = String::new();
    let mut in_code = false;
    let mut code_content = String::new();
    let mut code_lang = String::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("```") {
            if !in_code {
                // Opening fence — flush accumulated text first
                if !current_text.is_empty() {
                    blocks.push(ChatBlock::Text(std::mem::take(&mut current_text)));
                }
                code_lang = rest.trim().to_string();
                in_code = true;
            } else {
                // Closing fence — emit code block
                blocks.push(ChatBlock::Code {
                    lang: std::mem::take(&mut code_lang),
                    content: std::mem::take(&mut code_content),
                });
                in_code = false;
            }
        } else if in_code {
            code_content.push_str(line);
            code_content.push('\n');
        } else {
            current_text.push_str(line);
            current_text.push('\n');
        }
    }

    // Flush remaining content
    if in_code {
        blocks.push(ChatBlock::Code {
            lang: code_lang,
            content: code_content,
        });
    } else if !current_text.is_empty() {
        blocks.push(ChatBlock::Text(current_text));
    }

    blocks
}

// ─── AppMode ──────────────────────────────────────────────────────────────────

/// Modal input state — mirrors a minimal vim-style mode system.
#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    /// Navigation / scroll mode.
    Normal,
    /// Typing mode — characters go to the input buffer.
    Insert,
}

// ─── ToolStatus ───────────────────────────────────────────────────────────────

/// Status of a tool call rendered inline in the chat history.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolStatus {
    /// Tool is currently executing.
    Running,
    /// Tool completed successfully.
    Done,
    /// Tool completed with an error.
    Error,
}

// ─── Snippet truncation ───────────────────────────────────────────────────────

/// Maximum character count for an error snippet shown in a [`ChatEntry::ToolCall`].
pub const MAX_SNIPPET_CHARS: usize = 300;

/// Maximum line count for an error snippet shown in a [`ChatEntry::ToolCall`].
pub const MAX_SNIPPET_LINES: usize = 5;

/// Truncate a string to at most [`MAX_SNIPPET_CHARS`] characters and
/// [`MAX_SNIPPET_LINES`] lines, appending `…` when truncated.
pub fn truncate_snippet(s: &str) -> String {
    // Apply line limit first
    let line_limited: String = {
        let mut lines = s.lines();
        let mut kept = Vec::new();
        for _ in 0..MAX_SNIPPET_LINES {
            match lines.next() {
                Some(l) => kept.push(l),
                None => break,
            }
        }
        let truncated_by_lines = lines.next().is_some();
        let joined = kept.join("\n");
        if truncated_by_lines {
            format!("{joined}…")
        } else {
            joined
        }
    };

    // Apply char limit
    if line_limited.chars().count() > MAX_SNIPPET_CHARS {
        let truncated: String = line_limited.chars().take(MAX_SNIPPET_CHARS).collect();
        format!("{truncated}…")
    } else {
        line_limited
    }
}

// ─── ToolEntry ────────────────────────────────────────────────────────────────

/// Structured representation of a single tool invocation for the tools panel.
pub struct ToolEntry {
    /// Tool name (e.g. `"bash"`, `"read"`).
    pub name: String,
    /// Tool params serialised as a compact JSON string.
    pub params: String,
    /// Tool result once the call has completed; `None` while running.
    pub result: Option<String>,
    /// Whether the tool returned an error result.
    pub is_error: bool,
    /// Whether the entry is expanded in the panel (shows full params/result).
    pub expanded: bool,
}

// ─── TuiApp ───────────────────────────────────────────────────────────────────

/// The top-level TUI application struct.
///
/// Holds all rendering state plus [`Arc`] handles for the provider, tools,
/// middleware, and the current [`Conversation`].  The conversation is wrapped
/// in `Arc<tokio::sync::Mutex<Conversation>>` so the spawned turn task can
/// update it while the UI continues to render.
pub struct TuiApp {
    /// Active color theme.
    pub theme: Theme,

    /// Current input mode.
    pub mode: AppMode,

    /// Structured conversation history.
    pub chat_history: Vec<ChatEntry>,

    /// Structured tool activity entries for the right-hand panel.
    pub tool_entries: Vec<ToolEntry>,

    /// Index of the currently selected tool entry, if any.
    pub selected_tool: Option<usize>,

    /// Live input buffer (what the user is currently typing).
    pub input_buffer: String,

    /// How many lines the conversation pane is scrolled down.
    pub scroll_offset: usize,

    /// Whether the conversation pane is pinned to the bottom (auto-scrolls).
    ///
    /// `true` on startup and whenever the user presses `G`.  Pressing `j` or
    /// `k` sets it to `false` so the user can freely scroll without being
    /// snapped back on every new chunk.
    pub scroll_pinned: bool,

    /// Whether the help overlay is visible.
    pub show_help: bool,

    /// Model name shown in the status bar.
    pub model_name: String,

    /// Total completed turn count (for status bar).
    pub conversation_messages: usize,

    /// Whether a turn is in progress (disables submit).
    pub is_waiting: bool,

    /// Accumulated input token count across all turns.
    pub total_input_tokens: u32,

    /// Accumulated output token count across all turns.
    pub total_output_tokens: u32,

    /// Most recent input token count (from the last Usage event).
    /// Not cumulative — replaced on each `TurnEvent::Usage`.
    pub last_input_tokens: u32,

    /// Optional context token limit for compression (from `AppConfig`).
    pub context_limit: Option<u32>,

    /// Sender side of the UI event channel (for the spawned turn task).
    ui_tx: mpsc::Sender<TurnEvent>,

    /// Receiver side of the UI event channel.
    ui_rx: Option<mpsc::Receiver<TurnEvent>>,

    /// Abort sender — `Some` while a turn is in progress, `None` when idle.
    /// Sending on this channel signals the turn task to cancel.
    pub abort_tx: Option<oneshot::Sender<()>>,

    /// Shared conversation state — updated by the spawned turn task.
    conv: Arc<tokio::sync::Mutex<Conversation>>,

    /// LLM provider (shared with spawned tasks).
    provider: Arc<dyn Provider>,

    /// Tool registry (shared with spawned tasks).
    tools: Arc<ToolRegistry>,

    /// Middleware chain (shared with spawned tasks).
    middleware: Arc<Middleware>,
}

impl TuiApp {
    /// Create a new [`TuiApp`].
    ///
    /// Call [`run`](TuiApp::run) to enter raw mode and start the event loop.
    pub fn new(
        conv: Arc<tokio::sync::Mutex<Conversation>>,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        middleware: Arc<Middleware>,
        model_name: String,
        context_limit: Option<u32>,
    ) -> Result<Self> {
        let (ui_tx, ui_rx) = mpsc::channel(256);
        Ok(Self {
            theme: Theme::default(),
            mode: AppMode::Normal,
            chat_history: Vec::new(),
            tool_entries: Vec::new(),
            selected_tool: None,
            input_buffer: String::new(),
            scroll_offset: 0,
            scroll_pinned: true,
            show_help: false,
            model_name,
            conversation_messages: 0,
            is_waiting: false,
            total_input_tokens: 0,
            total_output_tokens: 0,
            last_input_tokens: 0,
            context_limit,
            ui_tx,
            ui_rx: Some(ui_rx),
            abort_tx: None,
            conv,
            provider,
            tools,
            middleware,
        })
    }

    /// Headless constructor used in unit tests — no terminal I/O, no real provider.
    #[cfg(test)]
    pub fn headless() -> Self {
        Self::headless_with_limit(None)
    }

    /// Headless constructor with an explicit context limit — for tests that need to exercise
    /// the compression UI path. `headless()` delegates here with `None`.
    #[cfg(test)]
    pub fn headless_with_limit(context_limit: Option<u32>) -> Self {
        use crate::config::AppConfig;

        let (ui_tx, ui_rx) = mpsc::channel(256);

        // Minimal stub provider for tests — never called in unit tests
        struct StubProvider;
        impl Provider for StubProvider {
            fn stream_completion<'a>(
                &'a self,
                _messages: &'a [crate::provider::Message],
                _tools: &'a [serde_json::Value],
                _system_prompt: Option<&'a str>,
            ) -> futures::stream::BoxStream<'a, Result<crate::provider::StreamEvent, crate::provider::ProviderError>> {
                Box::pin(futures::stream::empty())
            }
        }

        let conv = Arc::new(tokio::sync::Mutex::new(Conversation::new(
            "test-id",
            "test-model",
            AppConfig::default(),
        )));

        Self {
            theme: Theme::default(),
            mode: AppMode::Normal,
            chat_history: Vec::new(),
            tool_entries: Vec::new(),
            selected_tool: None,
            input_buffer: String::new(),
            scroll_offset: 0,
            scroll_pinned: true,
            show_help: false,
            model_name: "test-model".to_string(),
            conversation_messages: 0,
            is_waiting: false,
            total_input_tokens: 0,
            total_output_tokens: 0,
            last_input_tokens: 0,
            context_limit,
            ui_tx,
            ui_rx: Some(ui_rx),
            abort_tx: None,
            conv,
            provider: Arc::new(StubProvider),
            tools: Arc::new(ToolRegistry::new()),
            middleware: Arc::new(crate::types::Middleware::default()),
        }
    }

    /// Main event loop.  Initialises the terminal, then runs until the user
    /// quits.
    pub async fn run(&mut self) -> Result<()> {
        // Install panic hook so the terminal is restored on panic
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stderr(), LeaveAlternateScreen);
            original_hook(info);
        }));

        // Enter raw / alternate-screen mode
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;

        let mut event_stream = EventStream::new();
        let mut ui_rx = self
            .ui_rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("run() called without ui_rx (double-call?)"))?;

        let result = self.event_loop(&mut terminal, &mut event_stream, &mut ui_rx).await;

        // Always clean up terminal even if event loop returned an error
        let _ = cleanup_terminal(&mut terminal);
        result
    }

    /// Inner event loop — separated so we can always run cleanup after it.
    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
        event_stream: &mut EventStream,
        ui_rx: &mut mpsc::Receiver<TurnEvent>,
    ) -> Result<()> {
        loop {
            // Render before waiting for the next event
            terminal.draw(|f| ui::render(f, self))?;

            tokio::select! {
                // ── Keyboard / terminal events ──────────────────────────────
                maybe_event = event_stream.next() => {
                    match maybe_event {
                        Some(Ok(crossterm::event::Event::Key(key))) => {
                            let action = events::handle_key_event(key, self);
                            match action {
                                events::Action::Quit => break,
                                events::Action::Submit(input) => {
                                    self.handle_submit(input).await;
                                }
                                events::Action::Cancel => {
                                    self.handle_cancel();
                                }
                                events::Action::None => {}
                            }
                        }
                        Some(Ok(_)) => {} // resize, focus, mouse — ignore
                        Some(Err(_)) => {} // transient I/O errors — ignore
                        None => break,    // EventStream exhausted (terminal gone)
                    }
                }

                // ── Turn events ─────────────────────────────────────────────
                maybe_event = ui_rx.recv() => {
                    match maybe_event {
                        Some(event) => self.handle_ui_event(event),
                        None => {
                            // Channel closed — turn has exited; keep TUI open
                            // so the user can read the last output.
                        }
                    }
                }
            }
        }

        Ok(())
    }

    // ─── Private helpers ─────────────────────────────────────────────────────

    /// Cancel the current in-progress turn by draining `abort_tx`.
    ///
    /// If `abort_tx` is `Some`, the sender is consumed via `take()` and `()`
    /// is sent on the channel to signal cancellation.  If `abort_tx` is
    /// `None` (no turn in progress) this is a no-op.
    fn handle_cancel(&mut self) {
        if let Some(tx) = self.abort_tx.take() {
            let _ = tx.send(());
        }
        // turn() doesn't support cancellation yet — no further action needed
    }

    /// Handle a submitted input line.
    async fn handle_submit(&mut self, input: String) {
        let trimmed = input.trim().to_string();
        if trimmed.is_empty() {
            return;
        }

        // Echo the user message into the conversation pane
        self.chat_history.push(ChatEntry::User(trimmed.clone()));
        self.is_waiting = true;

        // Store abort sender for potential cancellation (abort_rx discarded
        // until turn() supports cancellation in a future step)
        let (abort_tx, _abort_rx) = oneshot::channel::<()>();
        self.abort_tx = Some(abort_tx);

        // Clone Arc handles for the spawned task
        let conv_arc = Arc::clone(&self.conv);
        let provider = Arc::clone(&self.provider);
        let tools = Arc::clone(&self.tools);
        let middleware = Arc::clone(&self.middleware);
        let tx = self.ui_tx.clone();
        // Capture Copy scalars for context compression (avoids borrowing self in async block)
        let context_limit = self.context_limit;
        let keep_recent = {
            let conv = self.conv.try_lock().map(|c| c.config.context.keep_recent);
            conv.unwrap_or(20)
        };
        let threshold = {
            let conv = self.conv.try_lock().map(|c| c.config.context.threshold);
            conv.unwrap_or(0.8)
        };

        tokio::spawn(async move {
            // Snapshot + append user message (pure, doesn't mutate Arc)
            let conv_with_msg = conv_arc.lock().await.clone().with_user_message(trimmed);

            // Conditionally compress context before the turn
            let conv_to_use = if let Some(limit) = context_limit {
                let config = ContextConfig { limit: Some(limit), keep_recent, threshold };
                match maybe_compress_context(conv_with_msg, &config, &*provider).await {
                    Ok((c, Some(evt))) => {
                        tx.send(evt).await.ok();
                        c
                    }
                    Ok((c, None)) => c,
                    Err(e) => {
                        tx.send(TurnEvent::Error(e.to_string())).await.ok();
                        return;
                    }
                }
            } else {
                conv_with_msg
            };

            match turn(conv_to_use, &*provider, &tools, &middleware).await {
                Ok((new_conv, events)) => {
                    // Update shared conversation
                    *conv_arc.lock().await = new_conv;
                    // Forward all events to the UI channel
                    for event in events {
                        let _ = tx.send(event).await;
                    }
                }
                Err(e) => {
                    let _ = tx.send(TurnEvent::Error(e.to_string())).await;
                }
            }
        });
    }

    /// Apply a [`TurnEvent`] from the turn pipeline to TUI state.
    pub fn handle_ui_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::TextChunk(text) => {
                // Append to last AssistantStreaming entry, or push a new one
                if let Some(ChatEntry::AssistantStreaming(buf)) = self.chat_history.last_mut() {
                    buf.push_str(&text);
                } else {
                    self.chat_history.push(ChatEntry::AssistantStreaming(text));
                }
                // Auto-scroll to bottom when pinned
                if self.scroll_pinned {
                    self.scroll_offset = usize::MAX;
                }
            }
            TurnEvent::ToolStart { name, params } => {
                let params_str = params.to_string();
                // NEW: push inline ToolCall into chat history
                self.chat_history.push(ChatEntry::ToolCall {
                    name: name.clone(),
                    status: ToolStatus::Running,
                    output_snippet: None,
                });
                // LEGACY: keep tool_entries for existing events.rs / ui.rs
                self.tool_entries.push(ToolEntry {
                    name,
                    params: params_str,
                    result: None,
                    is_error: false,
                    expanded: false,
                });
                // Auto-select the new entry
                self.selected_tool = Some(self.tool_entries.len() - 1);
                // Auto-scroll to bottom when pinned
                if self.scroll_pinned {
                    self.scroll_offset = usize::MAX;
                }
            }
            TurnEvent::ToolComplete { name, result, is_error } => {
                // NEW: find and update matching ToolCall in chat_history
                if let Some(ChatEntry::ToolCall { status, output_snippet, .. }) = self
                    .chat_history
                    .iter_mut()
                    .rev()
                    .find(|e| matches!(e, ChatEntry::ToolCall { name: n, status: ToolStatus::Running, .. } if n == &name))
                {
                    *status = if is_error { ToolStatus::Error } else { ToolStatus::Done };
                    *output_snippet = if is_error {
                        Some(truncate_snippet(&result))
                    } else {
                        None
                    };
                }
                // LEGACY: keep tool_entries for existing events.rs / ui.rs
                if let Some(entry) = self
                    .tool_entries
                    .iter_mut()
                    .rev()
                    .find(|e| e.name == name && e.result.is_none())
                {
                    entry.result = Some(result);
                    entry.is_error = is_error;
                }
                // Auto-scroll to bottom when pinned
                if self.scroll_pinned {
                    self.scroll_offset = usize::MAX;
                }
            }
            TurnEvent::TurnEnd => {
                // Convert last AssistantStreaming into AssistantDone
                if let Some(entry) = self.chat_history.last_mut() {
                    if let ChatEntry::AssistantStreaming(text) = entry.clone() {
                        let blocks = parse_chat_blocks(&text);
                        *entry = ChatEntry::AssistantDone(blocks);
                    }
                }
                self.conversation_messages += 1;
                self.is_waiting = false;
                // TurnEnd finalises content — auto-scroll when pinned
                if self.scroll_pinned {
                    self.scroll_offset = usize::MAX;
                }
            }
            TurnEvent::Usage { input_tokens, output_tokens } => {
                self.total_input_tokens += input_tokens;
                self.total_output_tokens += output_tokens;
                self.last_input_tokens = input_tokens;
            }
            TurnEvent::Error(e) => {
                self.chat_history.push(ChatEntry::AssistantDone(vec![ChatBlock::Text(
                    format!("\n[Error] {e}\n"),
                )]));
                self.is_waiting = false;
            }
            TurnEvent::ContextSummarized { messages_before, messages_after, tokens_after, .. } => {
                self.chat_history.push(ChatEntry::AssistantDone(vec![ChatBlock::Text(
                    format!(
                        "\n[Context compressed: {messages_before} → {messages_after} messages]\n"
                    ),
                )]));
                self.last_input_tokens = tokens_after;
                if self.scroll_pinned {
                    self.scroll_offset = usize::MAX;
                }
            }
        }
    }
}

// ─── Terminal cleanup helper ──────────────────────────────────────────────────

fn cleanup_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn headless_app_initial_state() {
        let app = TuiApp::headless();
        assert!(app.chat_history.is_empty());
        assert!(app.input_buffer.is_empty());
        assert_eq!(app.scroll_offset, 0);
        assert!(app.scroll_pinned);
        assert!(!app.is_waiting);
        // abort_tx starts as None (no in-progress turn)
        assert!(app.abort_tx.is_none());
    }

    #[test]
    fn handle_ui_event_text_chunk_appends() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::TextChunk("Hello ".to_string()));
        app.handle_ui_event(TurnEvent::TextChunk("world".to_string()));
        // Two chunks — second appended to first entry (streaming)
        assert_eq!(app.chat_history.len(), 1);
        assert_eq!(
            app.chat_history[0],
            ChatEntry::AssistantStreaming("Hello world".to_string())
        );
    }

    // ─── New ToolStatus / ChatEntry::ToolCall tests ───────────────────────────

    #[test]
    fn tool_status_variants_derive_debug_clone_partialeq() {
        let a = ToolStatus::Running;
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(format!("{:?}", ToolStatus::Done), "Done");
        assert_ne!(ToolStatus::Running, ToolStatus::Error);
    }

    #[test]
    fn chat_entry_tool_call_variant_can_be_pushed_and_matched() {
        let mut v: Vec<ChatEntry> = Vec::new();
        v.push(ChatEntry::ToolCall {
            name: "bash".to_string(),
            status: ToolStatus::Running,
            output_snippet: None,
        });
        assert_eq!(v.len(), 1);
        match &v[0] {
            ChatEntry::ToolCall { name, status, output_snippet } => {
                assert_eq!(name, "bash");
                assert_eq!(*status, ToolStatus::Running);
                assert!(output_snippet.is_none());
            }
            _ => panic!("expected ToolCall variant"),
        }
    }

    #[test]
    fn tuiapp_headless_has_abort_tx_none() {
        let app = TuiApp::headless();
        assert!(app.abort_tx.is_none());
    }

    #[test]
    fn truncate_snippet_respects_char_limit() {
        let long_str: String = "a".repeat(400);
        let result = truncate_snippet(&long_str);
        // ≤303: 300 chars + 3-byte ellipsis (1 char '…')
        assert!(result.chars().count() <= MAX_SNIPPET_CHARS + 1);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_snippet_respects_line_limit() {
        let input = (0..10).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let result = truncate_snippet(&input);
        let line_count = result.lines().count();
        // Only first 5 lines retained (the … is on the last line after join)
        assert!(line_count <= MAX_SNIPPET_LINES, "got {line_count} lines");
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_snippet_does_not_truncate_short_string() {
        let input = "line1\nline2";
        let result = truncate_snippet(input);
        assert_eq!(result, input);
        assert!(!result.ends_with('…'));
    }

    #[test]
    fn handle_ui_event_tool_start_pushes_tool_call_entry() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({"command": "ls"}),
        });
        // Last entry in chat_history should be ToolCall{Running}
        match app.chat_history.last().expect("should have entry") {
            ChatEntry::ToolCall { name, status, output_snippet } => {
                assert_eq!(name, "bash");
                assert_eq!(*status, ToolStatus::Running);
                assert!(output_snippet.is_none());
            }
            _ => panic!("expected ChatEntry::ToolCall"),
        }
    }

    #[test]
    fn handle_ui_event_tool_complete_success_updates_to_done() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({}),
        });
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "bash".to_string(),
            result: "ok".to_string(),
            is_error: false,
        });
        match app.chat_history.last().expect("should have entry") {
            ChatEntry::ToolCall { status, output_snippet, .. } => {
                assert_eq!(*status, ToolStatus::Done);
                assert!(output_snippet.is_none(), "success: no snippet");
            }
            _ => panic!("expected ChatEntry::ToolCall"),
        }
    }

    #[test]
    fn handle_ui_event_tool_complete_error_updates_to_error_with_snippet() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({}),
        });
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "bash".to_string(),
            result: "error text".to_string(),
            is_error: true,
        });
        match app.chat_history.last().expect("should have entry") {
            ChatEntry::ToolCall { status, output_snippet, .. } => {
                assert_eq!(*status, ToolStatus::Error);
                assert_eq!(output_snippet.as_deref(), Some("error text"));
            }
            _ => panic!("expected ChatEntry::ToolCall"),
        }
    }

    #[test]
    fn handle_ui_event_tool_complete_unknown_name_is_noop() {
        let mut app = TuiApp::headless();
        // No prior ToolStart — complete for unknown name should not panic
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "unknown".to_string(),
            result: "x".to_string(),
            is_error: false,
        });
        assert!(app.chat_history.is_empty());
    }

    #[test]
    fn handle_ui_event_turn_end_increments_message_count() {
        let mut app = TuiApp::headless();
        app.is_waiting = true;
        app.handle_ui_event(TurnEvent::TurnEnd);
        assert_eq!(app.conversation_messages, 1);
        assert!(!app.is_waiting);
    }

    #[test]
    fn handle_ui_event_error_logged_to_messages() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::Error("oops".to_string()));
        assert!(!app.chat_history.is_empty());
        // Error becomes an AssistantDone entry with the error text
        match &app.chat_history[0] {
            ChatEntry::AssistantDone(blocks) => {
                let text = match &blocks[0] {
                    ChatBlock::Text(s) => s.clone(),
                    _ => panic!("expected Text block"),
                };
                assert!(text.contains("oops"));
            }
            _ => panic!("expected AssistantDone"),
        }
        assert!(!app.is_waiting);
    }

    #[test]
    fn turn_end_clears_waiting_state() {
        let mut app = TuiApp::headless();
        app.is_waiting = true;
        app.handle_ui_event(TurnEvent::TurnEnd);
        assert!(!app.is_waiting, "TurnEnd should clear is_waiting");
    }

    #[test]
    fn handle_ui_event_usage_accumulates() {
        let mut app = TuiApp::headless();
        assert_eq!(app.total_input_tokens, 0);
        assert_eq!(app.total_output_tokens, 0);

        app.handle_ui_event(TurnEvent::Usage { input_tokens: 100, output_tokens: 200 });
        app.handle_ui_event(TurnEvent::Usage { input_tokens: 100, output_tokens: 200 });

        assert_eq!(app.total_input_tokens, 200);
        assert_eq!(app.total_output_tokens, 400);
    }

    #[test]
    fn status_bar_cost_format() {
        // 1000 input @ $3/M + 2000 output @ $15/M = $0.0030 + $0.0300 = $0.0330
        let cost = (1000_f64 / 1_000_000.0) * 3.00 + (2000_f64 / 1_000_000.0) * 15.00;
        assert_eq!(format!("${:.4}", cost), "$0.0330");
    }

    // ─── Step 4: parse_chat_blocks tests ─────────────────────────────────────

    #[test]
    fn parse_chat_blocks_no_fence() {
        let blocks = parse_chat_blocks("hello world");
        assert_eq!(blocks, vec![ChatBlock::Text("hello world\n".to_string())]);
    }

    #[test]
    fn parse_chat_blocks_single_fence() {
        let input = "intro\n```\ncode\n```\n";
        let blocks = parse_chat_blocks(input);
        assert_eq!(
            blocks,
            vec![
                ChatBlock::Text("intro\n".to_string()),
                ChatBlock::Code { lang: "".to_string(), content: "code\n".to_string() },
            ]
        );
    }

    #[test]
    fn parse_chat_blocks_with_lang() {
        let input = "```rust\nfn main() {}\n```\n";
        let blocks = parse_chat_blocks(input);
        assert_eq!(
            blocks,
            vec![ChatBlock::Code { lang: "rust".to_string(), content: "fn main() {}\n".to_string() }]
        );
    }

    #[test]
    fn parse_chat_blocks_unclosed_fence() {
        let input = "intro\n```python\nsome code\n";
        let blocks = parse_chat_blocks(input);
        assert_eq!(
            blocks,
            vec![
                ChatBlock::Text("intro\n".to_string()),
                ChatBlock::Code { lang: "python".to_string(), content: "some code\n".to_string() },
            ]
        );
    }

    #[test]
    fn parse_chat_blocks_empty() {
        let blocks = parse_chat_blocks("");
        assert!(blocks.is_empty());
    }

    #[test]
    fn streaming_lifecycle_chunks_appended() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::TextChunk("Hello ".to_string()));
        app.handle_ui_event(TurnEvent::TextChunk("world".to_string()));
        assert_eq!(app.chat_history.len(), 1);
        assert_eq!(
            app.chat_history[0],
            ChatEntry::AssistantStreaming("Hello world".to_string())
        );
    }

    #[test]
    fn streaming_lifecycle_ends_as_done() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::TextChunk("Hello ".to_string()));
        app.handle_ui_event(TurnEvent::TextChunk("world".to_string()));
        app.handle_ui_event(TurnEvent::TurnEnd);
        assert_eq!(app.chat_history.len(), 1);
        assert_eq!(
            app.chat_history[0],
            ChatEntry::AssistantDone(vec![ChatBlock::Text("Hello world\n".to_string())])
        );
    }

    // ── Step 5: scroll_pinned auto-scroll anchor ──────────────────────────────

    #[test]
    fn headless_app_starts_pinned() {
        let app = TuiApp::headless();
        assert!(app.scroll_pinned, "should start pinned");
        assert!(app.abort_tx.is_none(), "abort_tx should be None on init");
    }

    #[test]
    fn text_chunk_auto_scrolls_when_pinned() {
        let mut app = TuiApp::headless();
        assert!(app.scroll_pinned);
        app.handle_ui_event(TurnEvent::TextChunk("hi".to_string()));
        assert_eq!(app.scroll_offset, usize::MAX, "pinned: offset should jump to MAX on new content");
    }

    #[test]
    fn text_chunk_no_auto_scroll_when_unpinned() {
        let mut app = TuiApp::headless();
        app.scroll_pinned = false;
        app.scroll_offset = 10;
        app.handle_ui_event(TurnEvent::TextChunk("hi".to_string()));
        assert_eq!(app.scroll_offset, 10, "unpinned: offset should not change on new content");
    }

    #[test]
    fn tool_start_auto_scrolls_when_pinned() {
        let mut app = TuiApp::headless();
        assert!(app.scroll_pinned);
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({}),
        });
        assert_eq!(app.scroll_offset, usize::MAX);
        // ToolStart should push a ChatEntry::ToolCall into chat_history
        assert!(
            matches!(app.chat_history.last(), Some(ChatEntry::ToolCall { .. })),
            "ToolStart should append ChatEntry::ToolCall to chat_history"
        );
    }

    #[test]
    fn tool_complete_auto_scrolls_when_pinned() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({}),
        });
        app.scroll_offset = 5; // reset to simulate user scrolled then G
        app.scroll_pinned = true;
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "bash".to_string(),
            result: "done".to_string(),
            is_error: false,
        });
        assert_eq!(app.scroll_offset, usize::MAX);
        // ToolComplete should update the ChatEntry::ToolCall to Done
        assert!(
            matches!(
                app.chat_history.last(),
                Some(ChatEntry::ToolCall { status: ToolStatus::Done, .. })
            ),
            "ToolComplete (success) should set status to Done"
        );
    }

    #[test]
    fn turn_end_auto_scrolls_when_pinned() {
        let mut app = TuiApp::headless();
        // Simulate a streaming assistant message then end
        app.handle_ui_event(TurnEvent::TextChunk("hello".to_string()));
        app.scroll_offset = 5; // simulate user scrolled mid-stream then G re-pins
        app.scroll_pinned = true;
        app.handle_ui_event(TurnEvent::TurnEnd);
        assert_eq!(
            app.scroll_offset,
            usize::MAX,
            "TurnEnd should auto-scroll to MAX when pinned"
        );
    }

    #[test]
    fn turn_end_no_auto_scroll_when_unpinned() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::TextChunk("hello".to_string()));
        app.scroll_pinned = false;
        app.scroll_offset = 7;
        app.handle_ui_event(TurnEvent::TurnEnd);
        assert_eq!(
            app.scroll_offset, 7,
            "TurnEnd should not change scroll_offset when unpinned"
        );
    }

    // ─── Step 4: ContextSummarized + last_input_tokens ───────────────────────

    #[test]
    fn handle_ui_event_context_summarized_appends_notice() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ContextSummarized {
            messages_before: 10,
            messages_after: 3,
            tokens_before: 5000,
            tokens_after: 500,
        });
        assert_eq!(app.chat_history.len(), 1, "ContextSummarized should append exactly 1 entry");
    }

    #[test]
    fn handle_ui_event_usage_updates_last_input_tokens() {
        let mut app = TuiApp::headless();
        assert_eq!(app.last_input_tokens, 0);
        app.handle_ui_event(TurnEvent::Usage { input_tokens: 5000, output_tokens: 100 });
        assert_eq!(app.last_input_tokens, 5000);
    }

    #[test]
    fn handle_ui_event_usage_still_accumulates_totals() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::Usage { input_tokens: 3000, output_tokens: 100 });
        app.handle_ui_event(TurnEvent::Usage { input_tokens: 4000, output_tokens: 200 });
        assert_eq!(app.total_input_tokens, 7000, "total_input_tokens should accumulate");
        assert_eq!(app.last_input_tokens, 4000, "last_input_tokens should be the most recent value");
    }

    #[test]
    fn handle_ui_event_context_summarized_sets_last_input_tokens() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ContextSummarized {
            messages_before: 10,
            messages_after: 3,
            tokens_before: 5000,
            tokens_after: 500,
        });
        assert_eq!(app.last_input_tokens, 500, "ContextSummarized should update last_input_tokens to tokens_after");
    }

    #[test]
    fn tuiapp_new_stores_context_limit() {
        let app = TuiApp::headless_with_limit(Some(50_000));
        assert_eq!(app.context_limit, Some(50_000));
    }

    #[test]
    fn headless_with_limit_none_matches_headless() {
        let app = TuiApp::headless_with_limit(None);
        assert_eq!(app.context_limit, None);
    }

    // ─── Step 3: event loop wiring tests ─────────────────────────────────────

    #[test]
    fn handle_cancel_drains_abort_tx() {
        let mut app = TuiApp::headless();
        let (tx, mut rx) = oneshot::channel::<()>();
        app.abort_tx = Some(tx);
        app.handle_cancel();
        // abort_tx must be None after call
        assert!(app.abort_tx.is_none(), "handle_cancel must take() the sender");
        // The channel must have received the signal
        assert!(
            rx.try_recv().is_ok(),
            "handle_cancel must send () on the channel"
        );
    }

    #[test]
    fn handle_cancel_is_noop_when_abort_tx_is_none() {
        let mut app = TuiApp::headless();
        assert!(app.abort_tx.is_none());
        // Should not panic
        app.handle_cancel();
        assert!(app.abort_tx.is_none());
    }

    #[tokio::test]
    async fn handle_submit_help_command_no_longer_sets_show_help() {
        let mut app = TuiApp::headless();
        // /help should no longer be a special case — it should be submitted as a
        // regular user message (or discarded if trimmed is non-empty but just a
        // slash command — the point is show_help is NOT set)
        app.handle_submit("/help".to_string()).await;
        // show_help still exists as a legacy field but must not be set to true by /help
        assert!(!app.show_help, "/help must no longer set show_help");
    }
}
