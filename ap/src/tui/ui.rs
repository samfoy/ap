//! `tui/ui.rs` — Ratatui layout and rendering.
//!
//! Pure rendering function: takes a [`Frame`] and [`TuiApp`] reference, draws
//! the four-pane layout (status bar, conversation, tool panel, input box) and
//! the optional help overlay.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::tui::{AppMode, TuiApp};

/// Render the full TUI into `frame`.
pub fn render(frame: &mut Frame, app: &TuiApp) {
    // ── Outer layout: status (1 line) + main + input (3 lines) ──────────────
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(frame.area());

    render_status_bar(frame, app, outer[0]);
    render_main_area(frame, app, outer[1]);
    render_input_box(frame, app, outer[2]);

    // ── Help overlay (drawn on top) ──────────────────────────────────────────
    if app.show_help {
        render_help_overlay(frame);
    }
}

// ─── Status bar ───────────────────────────────────────────────────────────────

fn render_status_bar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let mode_label = match app.mode {
        AppMode::Normal => "NORMAL",
        AppMode::Insert => "INSERT",
    };
    let text = format!(
        " ap │ Model: {} │ Mode: {} │ Messages: {}",
        app.model_name,
        mode_label,
        app.conversation_messages,
    );
    let status = Paragraph::new(text).style(
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(status, area);
}

// ─── Main area ────────────────────────────────────────────────────────────────

fn render_main_area(frame: &mut Frame, app: &TuiApp, area: Rect) {
    // 65% conversation, 35% tools
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    render_conversation(frame, app, cols[0]);
    render_tool_panel(frame, app, cols[1]);
}

fn render_conversation(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let text = app.conversation.join("");
    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Conversation"))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));
    frame.render_widget(para, area);
}

fn render_tool_panel(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let text = app.tool_events.join("\n");
    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Tools"))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

// ─── Input box ────────────────────────────────────────────────────────────────

fn render_input_box(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let title = match app.mode {
        AppMode::Normal => "Input  [i=insert  j/k=scroll  Ctrl+C=quit  /help<Enter>=help]",
        AppMode::Insert => "Input  [Esc=normal  Enter=send  Ctrl+C=quit]",
    };
    let border_style = match app.mode {
        AppMode::Normal => Style::default().fg(Color::Gray),
        AppMode::Insert => Style::default().fg(Color::Yellow),
    };
    let para = Paragraph::new(app.input_buffer.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);

    // Show cursor inside input box when in Insert mode
    if matches!(app.mode, AppMode::Insert) {
        // Cursor position: inside the block (1 char border) + text length
        let x = area.x + 1 + app.input_buffer.len() as u16;
        let y = area.y + 1;
        // Clamp to avoid overflow
        if x < area.x + area.width.saturating_sub(1) {
            frame.set_cursor_position((x, y));
        }
    }
}

// ─── Help overlay ─────────────────────────────────────────────────────────────

fn render_help_overlay(frame: &mut Frame) {
    let area = centered_rect(60, 60, frame.area());

    // Clear the area behind the overlay so it looks like a modal
    frame.render_widget(Clear, area);

    let help_text = vec![
        Line::from(vec![Span::styled(
            " Key Bindings",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  i / Enter  ", Style::default().fg(Color::Green)),
            Span::raw("Enter Insert mode"),
        ]),
        Line::from(vec![
            Span::styled("  Esc        ", Style::default().fg(Color::Green)),
            Span::raw("Return to Normal mode"),
        ]),
        Line::from(vec![
            Span::styled("  Enter      ", Style::default().fg(Color::Green)),
            Span::raw("Send message (Insert mode)"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+C     ", Style::default().fg(Color::Red)),
            Span::raw("Quit"),
        ]),
        Line::from(vec![
            Span::styled("  j / PageDn ", Style::default().fg(Color::Cyan)),
            Span::raw("Scroll conversation down"),
        ]),
        Line::from(vec![
            Span::styled("  k / PageUp ", Style::default().fg(Color::Cyan)),
            Span::raw("Scroll conversation up"),
        ]),
        Line::from(vec![
            Span::styled("  /help      ", Style::default().fg(Color::Cyan)),
            Span::raw("Show this help (type + Enter)"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Press Esc to close",
            Style::default().fg(Color::Gray),
        )]),
    ];

    let para = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .alignment(Alignment::Left);

    frame.render_widget(para, area);
}

/// Return a rectangle centered within `r` at the given percentage dimensions.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
