---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: Ratatui TUI

## Description
Implement `src/tui/mod.rs`, `src/tui/ui.rs`, and `src/tui/events.rs` with a full ratatui TUI layout: status bar, conversation pane (scrollable), tool activity panel, and multiline input box. Mode state machine: Normal/Insert. `/help` overlay. Async integration via `tokio::select!` on keyboard events and `mpsc::Receiver<UiEvent>`.

## Background
The TUI is the primary user interface for `ap`. The key challenge is async integration: ratatui's terminal event poll must not block the tokio runtime while the agent streams responses. The solution is `tokio::select!` between a keyboard events task and the `UiEvent` receiver. TUI has no automated tests — a manual smoke test is the gate.

The TUI receives `UiEvent`s from the `AgentLoop` via a `mpsc::Receiver<UiEvent>` channel. When a `TextChunk` arrives, it's appended to the conversation buffer. `ToolStart`/`ToolComplete` go to the tool panel.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (Step 9)

**Note:** You MUST read the design document before beginning implementation. Section 4.12 covers the TUI design. Constraint E covers the async integration pattern.

## Technical Requirements
1. `src/tui/mod.rs`:
   - `TuiApp` struct: holds `Terminal<CrosstermBackend<Stdout>>`, `AppMode` enum (Normal/Insert), `conversation: Vec<String>`, `tool_events: Vec<String>`, `input_buffer: String`, `scroll_offset: usize`, `show_help: bool`
   - `TuiApp::new(ui_rx: mpsc::Receiver<UiEvent>, agent_loop: AgentLoop) -> anyhow::Result<Self>`
   - `TuiApp::run(&mut self) -> anyhow::Result<()>` — main event loop
   - Terminal setup: `enable_raw_mode()`, `EnterAlternateScreen`; cleanup on quit: `disable_raw_mode()`, `LeaveAlternateScreen`
   - Panic hook: restore terminal on panic (use `std::panic::set_hook` or `better-panic` crate)
2. `src/tui/ui.rs` — `render(frame: &mut Frame, app: &TuiApp)`:
   - Layout: `Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(3)])` → status bar (top), main area (middle), input box (bottom)
   - Main area: `Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)])` → conversation pane (left), tool panel (right)
   - Status bar: model name, provider, estimated token count (placeholder)
   - Conversation pane: `Paragraph` widget, scrollable via `scroll_offset`, `Block::bordered().title("Conversation")`
   - Tool panel: `Paragraph` widget with tool activity log, `Block::bordered().title("Tools")`
   - Input box: `Paragraph` widget with current `input_buffer`, `Block::bordered().title("Input [i=insert, Esc=normal, Enter=send, /help]")`
   - In Insert mode: show cursor at end of input
   - Help overlay: modal `Block` centered over the screen listing keybindings
3. `src/tui/events.rs` — event handling:
   - `handle_key_event(key: KeyEvent, app: &mut TuiApp) -> Action` enum: `None`, `Submit(String)`, `Quit`
   - Normal mode: `i` or `Enter` → Insert mode; `Ctrl+C` → Quit; `/` (typed) → check for `/help`
   - Insert mode: `Esc` → Normal; `Enter` → Submit(input_buffer.drain()); `Ctrl+C` → Quit; characters → append to input_buffer
   - Scrolling: `j`/`k` or `PageDown`/`PageUp` in Normal mode → adjust `scroll_offset`
4. Async event loop in `run()`:
   - Use `tokio::select!` between:
     - `crossterm::event::EventStream` (keyboard input) — from `crossterm::event::EventStream`
     - `ui_rx.recv()` (UiEvents from agent) — text chunks, tool events, turn end
   - On `UiEvent::TextChunk`: append to `conversation` buffer, re-render
   - On `UiEvent::ToolStart`: append to `tool_events`, re-render
   - On `Action::Submit(input)`: spawn `AgentLoop::run_turn(input)` in background tokio task
   - On `Action::Quit`: break loop, cleanup terminal

## Dependencies
- Task 01 (project scaffold) — `ratatui`, `crossterm` declared
- Task 07 (agent loop) — `AgentLoop`, `UiEvent`

## Implementation Approach
1. No automated tests for TUI — skip RED phase for this step
2. Implement `render()` in `ui.rs` first (pure function, easiest to reason about)
3. Implement `handle_key_event()` in `events.rs`
4. Implement `TuiApp::run()` with `tokio::select!` async loop
5. Wire terminal setup/teardown and panic hook
6. Manual smoke test: run `ap`, verify layout renders, input works, `/help` shows overlay, Ctrl+C quits cleanly

## Acceptance Criteria

1. **TUI Renders Without Crash**
   - Given `ap` is run with no arguments in a real terminal
   - When the TUI initializes
   - Then the ratatui layout renders: status bar, conversation pane, tool panel, input box — no crash

2. **Insert Mode Accepts Input**
   - Given the TUI is in Normal mode
   - When `i` is pressed
   - Then the TUI enters Insert mode and typed characters appear in the input box

3. **Help Overlay Shows Keybindings**
   - Given the TUI is running
   - When `/help` is typed and Enter pressed (or a dedicated help key)
   - Then a modal overlay appears listing at minimum: `i/Enter=insert`, `Esc=normal`, `Ctrl+C=quit`

4. **Ctrl+C Quits Cleanly**
   - Given the TUI is running
   - When `Ctrl+C` is pressed
   - Then the terminal is restored to normal state (raw mode disabled, alternate screen exited) — no leftover artifacts

5. **Conversation Pane Updates with Agent Response**
   - Given the TUI is running and a mock UiEvent::TextChunk is sent
   - When the render loop processes it
   - Then the text appears in the conversation pane on the next render

## Metadata
- **Complexity**: High
- **Labels**: tui, ratatui, crossterm, async, ui
- **Required Skills**: Rust, ratatui, crossterm, tokio select, async event loop
