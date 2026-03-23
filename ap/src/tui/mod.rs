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
use tokio::sync::mpsc;

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

    /// Handle a submitted input line.
    async fn handle_submit(&mut self, input: String) {
        let trimmed = input.trim().to_string();
        if trimmed.is_empty() {
            return;
        }

        if trimmed == "/help" {
            self.show_help = true;
            return;
        }

        // Echo the user message into the conversation pane
        self.chat_history.push(ChatEntry::User(trimmed.clone()));
        self.is_waiting = true;

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
                // Fill result on the last matching entry with result=None
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
    fn headless_app_starts_in_normal_mode() {
        let app = TuiApp::headless();
        assert_eq!(app.mode, AppMode::Normal);
        assert!(app.chat_history.is_empty());
        assert!(app.tool_entries.is_empty());
        assert!(app.selected_tool.is_none());
        assert!(app.input_buffer.is_empty());
        assert_eq!(app.scroll_offset, 0);
        assert!(app.scroll_pinned);
        assert!(!app.show_help);
        assert!(!app.is_waiting);
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

    #[test]
    fn handle_ui_event_tool_start_logged() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({"command": "ls"}),
        });
        assert_eq!(app.tool_entries.len(), 1);
        assert!(app.tool_entries[0].name.contains("bash"));
    }

    #[test]
    fn handle_ui_event_tool_complete_ok_logged() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "read".to_string(),
            params: serde_json::json!({}),
        });
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "read".to_string(),
            result: "file contents".to_string(),
            is_error: false,
        });
        assert_eq!(app.tool_entries.len(), 1);
        assert!(!app.tool_entries[0].is_error);
        assert_eq!(app.tool_entries[0].result, Some("file contents".to_string()));
    }

    #[test]
    fn handle_ui_event_tool_complete_err_logged() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({}),
        });
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "bash".to_string(),
            result: "error: something went wrong".to_string(),
            is_error: true,
        });
        assert!(app.tool_entries[0].is_error);
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

    // ─── Step 3: Structured ToolEntry tests ──────────────────────────────────

    #[test]
    fn tool_entry_start_creates_running_entry() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "read".to_string(),
            params: serde_json::json!({}),
        });
        assert_eq!(app.tool_entries.len(), 1);
        let entry = &app.tool_entries[0];
        assert_eq!(entry.name, "read");
        assert_eq!(entry.params, "{}");
        assert!(entry.result.is_none());
        assert!(!entry.is_error);
        assert!(!entry.expanded);
    }

    #[test]
    fn tool_entry_complete_fills_result() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "read".to_string(),
            params: serde_json::json!({}),
        });
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "read".to_string(),
            result: "contents".to_string(),
            is_error: false,
        });
        let entry = &app.tool_entries[0];
        assert_eq!(entry.result, Some("contents".to_string()));
        assert!(!entry.is_error);
    }

    #[test]
    fn tool_entry_is_error_from_turn_event() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({}),
        });
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "bash".to_string(),
            result: "error msg".to_string(),
            is_error: true,
        });
        let entry = &app.tool_entries[0];
        assert!(entry.is_error);
    }

    #[test]
    fn tool_entry_expand_toggle() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({}),
        });
        app.selected_tool = Some(0);
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        events::handle_key_event(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE), &mut app);
        assert!(app.tool_entries[0].expanded);
        // toggle again
        events::handle_key_event(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE), &mut app);
        assert!(!app.tool_entries[0].expanded);
    }

    #[test]
    fn tool_selection_bracket_keys() {
        let mut app = TuiApp::headless();
        for name in ["a", "b", "c"] {
            app.handle_ui_event(TurnEvent::ToolStart {
                name: name.to_string(),
                params: serde_json::json!({}),
            });
        }
        app.selected_tool = Some(1);
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        events::handle_key_event(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE), &mut app);
        assert_eq!(app.selected_tool, Some(2));
        // At end: ] stays at last
        events::handle_key_event(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE), &mut app);
        assert_eq!(app.selected_tool, Some(2));
        // [ moves back
        events::handle_key_event(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE), &mut app);
        assert_eq!(app.selected_tool, Some(1));
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
}
