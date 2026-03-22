//! `tui/mod.rs` — Ratatui TUI application state and main event loop.
//!
//! [`TuiApp`] owns the agent loop and UI event receiver.  Its
//! [`run`](TuiApp::run) method drives the `tokio::select!` loop between
//! crossterm keyboard events and agent [`UiEvent`]s.
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

use crate::app::{AgentLoop, UiEvent};

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
/// Holds all rendering state plus the async `AgentLoop` handle and event
/// receiver.  The agent loop runs concurrently in a spawned task; it sends
/// [`UiEvent`]s back via the `ui_rx` channel.
///
/// The ratatui [`Terminal`] is **not** stored here to avoid borrow conflicts
/// in [`run`](TuiApp::run): `terminal.draw()` requires `&mut Terminal` while
/// [`ui::render`] needs `&TuiApp`.
pub struct TuiApp {
    /// Current input mode.
    pub mode: AppMode,

    /// Streamed assistant text chunks; each string is a chunk.
    pub conversation: Vec<String>,

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

    /// Receiver for events from the agent loop.
    ui_rx: Option<mpsc::Receiver<UiEvent>>,

    /// The agent loop, wrapped so it can be shared with spawned tasks.
    agent: Option<Arc<tokio::sync::Mutex<AgentLoop>>>,
}

impl TuiApp {
    /// Create a new [`TuiApp`] without initialising the terminal.
    ///
    /// Call [`run`](TuiApp::run) to enter raw mode and start the event loop.
    pub fn new(
        ui_rx: mpsc::Receiver<UiEvent>,
        agent_loop: AgentLoop,
        model_name: String,
    ) -> Result<Self> {
        Ok(Self {
            mode: AppMode::Normal,
            conversation: Vec::new(),
            tool_events: Vec::new(),
            input_buffer: String::new(),
            scroll_offset: 0,
            show_help: false,
            model_name,
            conversation_messages: 0,
            ui_rx: Some(ui_rx),
            agent: Some(Arc::new(tokio::sync::Mutex::new(agent_loop))),
        })
    }

    /// Headless constructor used in unit tests — no terminal I/O.
    #[cfg(test)]
    pub fn headless() -> Self {
        Self {
            mode: AppMode::Normal,
            conversation: Vec::new(),
            tool_events: Vec::new(),
            input_buffer: String::new(),
            scroll_offset: 0,
            show_help: false,
            model_name: "test-model".to_string(),
            conversation_messages: 0,
            ui_rx: None,
            agent: None,
        }
    }

    /// Main event loop.  Initialises the terminal, then runs until the user
    /// quits.
    ///
    /// Uses `tokio::select!` to concurrently:
    /// - Poll crossterm keyboard events via [`EventStream`]
    /// - Receive [`UiEvent`]s from the running agent loop
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
        ui_rx: &mut mpsc::Receiver<UiEvent>,
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

                // ── Agent loop events ───────────────────────────────────────
                maybe_ui = ui_rx.recv() => {
                    match maybe_ui {
                        Some(event) => self.handle_ui_event(event),
                        None => {
                            // Channel closed — agent has exited; keep TUI open
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
        self.conversation
            .push(format!("\n[You] {trimmed}\n"));

        // Spawn agent turn in a background task so the TUI stays responsive
        if let Some(agent) = self.agent.as_ref().map(Arc::clone) {
            tokio::spawn(async move {
                let mut ag = agent.lock().await;
                if let Err(e) = ag.run_turn(trimmed).await {
                    eprintln!("ap: agent error: {e}");
                }
            });
        }
    }

    /// Apply a [`UiEvent`] from the agent loop to TUI state.
    pub fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::TextChunk(text) => {
                // Append to the last conversation entry (streaming)
                if let Some(last) = self.conversation.last_mut() {
                    last.push_str(&text);
                } else {
                    self.conversation.push(text);
                }
            }
            UiEvent::ToolStart { name, params } => {
                self.tool_events
                    .push(format!("⟳ {name}({})", params));
            }
            UiEvent::ToolComplete { name, result } => {
                let icon = if result.is_error { "✗" } else { "✓" };
                self.tool_events.push(format!("{icon} {name}"));
            }
            UiEvent::TurnEnd => {
                self.conversation.push("\n".to_string());
                self.conversation_messages += 1;
            }
            UiEvent::Error(e) => {
                self.conversation.push(format!("\n[Error] {e}\n"));
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
        assert!(app.conversation.is_empty());
        assert!(app.tool_events.is_empty());
        assert!(app.input_buffer.is_empty());
        assert_eq!(app.scroll_offset, 0);
        assert!(!app.show_help);
    }

    #[test]
    fn handle_ui_event_text_chunk_appends() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(UiEvent::TextChunk("Hello ".to_string()));
        app.handle_ui_event(UiEvent::TextChunk("world".to_string()));
        // Two chunks — second appended to first entry (streaming)
        assert_eq!(app.conversation.len(), 1);
        assert_eq!(app.conversation[0], "Hello world");
    }

    #[test]
    fn handle_ui_event_tool_start_logged() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(UiEvent::ToolStart {
            name: "bash".to_string(),
            params: serde_json::json!({"command": "ls"}),
        });
        assert_eq!(app.tool_events.len(), 1);
        assert!(app.tool_events[0].contains("bash"));
    }

    #[test]
    fn handle_ui_event_tool_complete_ok_logged() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(UiEvent::ToolComplete {
            name: "read".to_string(),
            result: crate::tools::ToolResult {
                content: "file contents".to_string(),
                is_error: false,
            },
        });
        assert_eq!(app.tool_events.len(), 1);
        assert!(app.tool_events[0].contains("✓"));
        assert!(app.tool_events[0].contains("read"));
    }

    #[test]
    fn handle_ui_event_tool_complete_err_logged() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(UiEvent::ToolComplete {
            name: "bash".to_string(),
            result: crate::tools::ToolResult {
                content: "error".to_string(),
                is_error: true,
            },
        });
        assert!(app.tool_events[0].contains("✗"));
    }

    #[test]
    fn handle_ui_event_turn_end_increments_message_count() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(UiEvent::TurnEnd);
        assert_eq!(app.conversation_messages, 1);
    }

    #[test]
    fn handle_ui_event_error_logged_to_conversation() {
        let mut app = TuiApp::headless();
        app.handle_ui_event(UiEvent::Error("oops".to_string()));
        assert!(!app.conversation.is_empty());
        assert!(app.conversation[0].contains("oops"));
    }
}
