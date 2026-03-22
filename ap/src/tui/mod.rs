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

pub mod events;
pub mod ui;

// ─── AppMode ──────────────────────────────────────────────────────────────────

/// Modal input state — mirrors a minimal vim-style mode system.
#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    /// Navigation / scroll mode.
    Normal,
    /// Typing mode — characters go to the input buffer.
    Insert,
}

// ─── TuiApp ───────────────────────────────────────────────────────────────────

/// The top-level TUI application struct.
///
/// Holds all rendering state plus [`Arc`] handles for the provider, tools,
/// middleware, and the current [`Conversation`].  The conversation is wrapped
/// in `Arc<tokio::sync::Mutex<Conversation>>` so the spawned turn task can
/// update it while the UI continues to render.
pub struct TuiApp {
    /// Current input mode.
    pub mode: AppMode,

    /// Streamed assistant text chunks; each string is a chunk.
    pub messages: Vec<String>,

    /// Tool activity entries for the right-hand panel.
    pub tool_events: Vec<String>,

    /// Live input buffer (what the user is currently typing).
    pub input_buffer: String,

    /// How many lines the conversation pane is scrolled down.
    pub scroll_offset: usize,

    /// Whether the help overlay is visible.
    pub show_help: bool,

    /// Model name shown in the status bar.
    pub model_name: String,

    /// Total completed turn count (for status bar).
    pub conversation_messages: usize,

    /// Whether a turn is in progress (disables submit).
    pub is_waiting: bool,

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
    ) -> Result<Self> {
        let (ui_tx, ui_rx) = mpsc::channel(256);
        Ok(Self {
            mode: AppMode::Normal,
            messages: Vec::new(),
            tool_events: Vec::new(),
            input_buffer: String::new(),
            scroll_offset: 0,
            show_help: false,
            model_name,
            conversation_messages: 0,
            is_waiting: false,
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
        use crate::config::AppConfig;

        let (ui_tx, ui_rx) = mpsc::channel(256);

        // Minimal stub provider for tests — never called in unit tests
        struct StubProvider;
        impl Provider for StubProvider {
            fn stream_completion<'a>(
                &'a self,
                _messages: &'a [crate::provider::Message],
                _tools: &'a [serde_json::Value],
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
            mode: AppMode::Normal,
            messages: Vec::new(),
            tool_events: Vec::new(),
            input_buffer: String::new(),
            scroll_offset: 0,
            show_help: false,
            model_name: "test-model".to_string(),
            conversation_messages: 0,
            is_waiting: false,
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
            .expect("run() called without ui_rx (double-call?)");

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

        // Echo the user message into the messages pane
        self.messages.push(format!("\n[You] {trimmed}\n"));
        self.is_waiting = true;

        // Clone Arc handles for the spawned task
        let conv_arc = Arc::clone(&self.conv);
        let provider = Arc::clone(&self.provider);
        let tools = Arc::clone(&self.tools);
        let middleware = Arc::clone(&self.middleware);
        let tx = self.ui_tx.clone();

        tokio::spawn(async move {
            // Snapshot + append user message (pure, doesn't mutate Arc)
            let c = conv_arc.lock().await.clone().with_user_message(trimmed);
            match turn(c, &*provider, &tools, &middleware).await {
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
                // Append to the last entry (streaming)
                if let Some(last) = self.messages.last_mut() {
                    last.push_str(&text);
                } else {
                    self.messages.push(text);
                }
            }
            TurnEvent::ToolStart { name, params } => {
                self.tool_events.push(format!("⟳ {name}({})", params));
            }
            TurnEvent::ToolComplete { name, result } => {
                // result is a String in the new TurnEvent — derive icon from content
                let icon = if result.starts_with("error") || result.contains("blocked") {
                    "✗"
                } else {
                    "✓"
                };
                self.tool_events.push(format!("{icon} {name}"));
            }
            TurnEvent::TurnEnd => {
                self.messages.push("\n".to_string());
                self.conversation_messages += 1;
                self.is_waiting = false;
            }
            TurnEvent::Error(e) => {
                self.messages.push(format!("\n[Error] {e}\n"));
                self.is_waiting = false;
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
mod tests {
    use super::*;

    #[test]
    fn headless_app_starts_in_normal_mode() {
        let app = TuiApp::headless();
        assert_eq!(app.mode, AppMode::Normal);
        assert!(app.messages.is_empty());
        assert!(app.tool_events.is_empty());
        assert!(app.input_buffer.is_empty());
        assert_eq!(app.scroll_offset, 0);
        assert!(!app.show_help);
        assert!(!app.is_waiting);
    }

    #[test]
    fn handle_ui_event_text_chunk_appends() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::TextChunk("Hello ".to_string()));
        app.handle_ui_event(TurnEvent::TextChunk("world".to_string()));
        // Two chunks — second appended to first entry (streaming)
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0], "Hello world");
    }

    #[test]
    fn handle_ui_event_tool_start_logged() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({"command": "ls"}),
        });
        assert_eq!(app.tool_events.len(), 1);
        assert!(app.tool_events[0].contains("bash"));
    }

    #[test]
    fn handle_ui_event_tool_complete_ok_logged() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "read".to_string(),
            result: "file contents".to_string(),
        });
        assert_eq!(app.tool_events.len(), 1);
        assert!(app.tool_events[0].contains("✓"));
        assert!(app.tool_events[0].contains("read"));
    }

    #[test]
    fn handle_ui_event_tool_complete_err_logged() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(TurnEvent::ToolComplete {
            name: "bash".to_string(),
            result: "error: something went wrong".to_string(),
        });
        assert!(app.tool_events[0].contains("✗"));
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
        assert!(!app.messages.is_empty());
        assert!(app.messages[0].contains("oops"));
        assert!(!app.is_waiting);
    }

    #[test]
    fn turn_end_clears_waiting_state() {
        let mut app = TuiApp::headless();
        app.is_waiting = true;
        app.handle_ui_event(TurnEvent::TurnEnd);
        assert!(!app.is_waiting, "TurnEnd should clear is_waiting");
    }
}
