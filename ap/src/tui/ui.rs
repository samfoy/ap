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

// ─── Pricing constants (Claude 3.5 Sonnet) ────────────────────────────────────

const COST_PER_M_INPUT: f64 = 3.00;
const COST_PER_M_OUTPUT: f64 = 15.00;

/// Calculate the height of the input box based on the number of lines in the buffer.
///
/// Content lines = newline count + 1, clamped to 2..=6.
/// Add 2 for borders → result in range 4..=8.
pub fn input_box_height(app: &TuiApp) -> u16 {
    let newlines = app.input_buffer.chars().filter(|&c| c == '\n').count();
    let content_lines = (newlines + 1).clamp(2, 6) as u16;
    content_lines + 2
}

/// Render the full TUI into `frame`.
pub fn render(frame: &mut Frame, app: &TuiApp) {
    // ── Outer layout: status (1 line) + main + input (dynamic height) ───────
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(input_box_height(app)),
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
    let input_k = app.total_input_tokens as f64 / 1_000.0;
    let output_k = app.total_output_tokens as f64 / 1_000.0;
    let cost = (app.total_input_tokens as f64 / 1_000_000.0) * COST_PER_M_INPUT
        + (app.total_output_tokens as f64 / 1_000_000.0) * COST_PER_M_OUTPUT;
    let text = format!(
        " ap │ {} │ {} │ Msgs: {} │ Tokens: ↑{:.1}k ↓{:.1}k │ Cost: ${:.4}",
        app.model_name,
        mode_label,
        app.conversation_messages,
        input_k,
        output_k,
        cost,
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

/// Convert a slice of [`ChatEntry`]s into ratatui [`Line`]s for rendering.
///
/// User entries are prefixed with a `[You]` label in Cyan bold. Code blocks are
/// styled with `bg(Color::Rgb(30, 30, 30)).fg(Color::White)` and wrapped in
/// Yellow header/footer lines. Used by [`render_conversation`] and unit tests.
pub fn chat_entries_to_lines<'a>(history: &'a [crate::tui::ChatEntry]) -> Vec<Line<'a>> {
    use crate::tui::{ChatBlock, ChatEntry};

    let mut lines: Vec<Line> = Vec::new();

    for entry in history {
        match entry {
            ChatEntry::User(text) => {
                // "[You]" prefix line in Cyan bold
                lines.push(Line::from(Span::styled(
                    "[You]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                for line in text.lines() {
                    lines.push(Line::from(line.to_string()));
                }
                lines.push(Line::from(""));
            }
            ChatEntry::AssistantStreaming(text) => {
                for line in text.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
            ChatEntry::AssistantDone(blocks) => {
                for block in blocks {
                    match block {
                        ChatBlock::Text(text) => {
                            for line in text.lines() {
                                lines.push(Line::from(line.to_string()));
                            }
                        }
                        ChatBlock::Code { lang, content } => {
                            let yellow = Style::default().fg(Color::Yellow);
                            // Header line in Yellow (line-level style so tests can check `.style`)
                            lines.push(Line::styled(format!(" ┌─ {lang} "), yellow));
                            // Body lines with dark bg + white fg
                            let code_style = Style::default()
                                .bg(Color::Rgb(30, 30, 30))
                                .fg(Color::White);
                            for line in content.lines() {
                                lines.push(Line::styled(line.to_string(), code_style));
                            }
                            // Footer line in Yellow
                            lines.push(Line::styled(" └────────", yellow));
                        }
                    }
                }
                lines.push(Line::from(""));
            }
        }
    }

    lines
}

fn render_conversation(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let lines = chat_entries_to_lines(&app.chat_history);

    use ratatui::text::Text;
    let para = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("Conversation"))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));
    frame.render_widget(para, area);
}

fn render_tool_panel(frame: &mut Frame, app: &TuiApp, area: Rect) {
    use ratatui::text::Text;

    let mut lines: Vec<Line> = Vec::new();

    for (i, entry) in app.tool_entries.iter().enumerate() {
        let is_selected = app.selected_tool == Some(i);
        let selection_marker = if is_selected { "▶ " } else { "  " };

        // Status icon
        let status_icon = match &entry.result {
            None => "⟳",
            Some(_) => {
                if entry.is_error { "✗" } else { "✓" }
            }
        };

        // Icon colour
        let icon_style = match &entry.result {
            None => Style::default().fg(Color::Yellow),
            Some(_) => {
                if entry.is_error {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                }
            }
        };

        let header_style = if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray)
        } else {
            Style::default()
        };

        // Collapsed header line
        let header = Line::from(vec![
            Span::styled(selection_marker.to_string(), header_style),
            Span::styled(status_icon.to_string(), icon_style),
            Span::styled(format!(" {}", entry.name), header_style),
        ]);
        lines.push(header);

        // Expanded detail lines
        if entry.expanded {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("params: ", Style::default().fg(Color::Gray)),
                Span::raw(entry.params.clone()),
            ]));
            if let Some(result) = &entry.result {
                let preview: String = result.chars().take(120).collect();
                let truncated = if result.len() > 120 {
                    format!("{}…", preview)
                } else {
                    preview
                };
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled("result: ", Style::default().fg(Color::Gray)),
                    Span::raw(truncated),
                ]));
            }
        }
    }

    let para = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("Tools  [ [/] select  e=expand ]"))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

// ─── Input box ────────────────────────────────────────────────────────────────

fn render_input_box(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let title = match app.mode {
        AppMode::Normal => "Input  [i=insert  j/k=scroll  G=bottom  Ctrl+C=quit  /help<Enter>=help]",
        AppMode::Insert => "Input  [Esc=normal  Enter=newline  Ctrl+Enter=send  Ctrl+C=quit]",
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
        // For multi-line buffers: x = chars after last \n, y = border + line index
        let buf = &app.input_buffer;
        let last_newline = buf.rfind('\n').map_or(0, |i| i + 1);
        let col = buf[last_newline..].len() as u16;
        let row = buf.chars().filter(|&c| c == '\n').count() as u16;
        let x = area.x + 1 + col;
        let y = area.y + 1 + row;
        // Clamp to avoid overflow
        if x < area.x + area.width.saturating_sub(1) && y < area.y + area.height.saturating_sub(1) {
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
            Span::raw("Insert newline (Insert mode)"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+Enter ", Style::default().fg(Color::Green)),
            Span::raw("Send message (Insert mode)"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+C     ", Style::default().fg(Color::Red)),
            Span::raw("Quit"),
        ]),
        Line::from(vec![
            Span::styled("  j / PageDn ", Style::default().fg(Color::Cyan)),
            Span::raw("Scroll conversation down (unpins auto-scroll)"),
        ]),
        Line::from(vec![
            Span::styled("  k / PageUp ", Style::default().fg(Color::Cyan)),
            Span::raw("Scroll conversation up (unpins auto-scroll)"),
        ]),
        Line::from(vec![
            Span::styled("  G          ", Style::default().fg(Color::Cyan)),
            Span::raw("Jump to bottom and re-pin auto-scroll"),
        ]),
        Line::from(vec![
            Span::styled("  [ / ]      ", Style::default().fg(Color::Cyan)),
            Span::raw("Select previous/next tool entry"),
        ]),
        Line::from(vec![
            Span::styled("  e          ", Style::default().fg(Color::Cyan)),
            Span::raw("Toggle expand selected tool entry"),
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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> TuiApp {
        TuiApp::headless()
    }

    #[test]
    fn input_box_height_min() {
        let app = make_app();
        assert_eq!(input_box_height(&app), 4);
    }

    #[test]
    fn input_box_height_grows_with_content() {
        let mut app = make_app();
        app.input_buffer = "a\nb\nc".to_string(); // 2 newlines → 3 lines
        assert_eq!(input_box_height(&app), 5); // 3 content + 2 borders
    }

    #[test]
    fn input_box_height_max() {
        let mut app = make_app();
        app.input_buffer = "\n".repeat(10); // 10 newlines → clamped to 6 content
        assert_eq!(input_box_height(&app), 8);
    }

    /// AC-11: Code block lines must render with `bg(Color::Rgb(30, 30, 30)).fg(Color::White)`.
    /// Header and footer lines must be Yellow; body lines have dark bg + white fg.
    #[test]
    fn code_block_lines_have_dark_bg_style() {
        use crate::tui::{ChatBlock, ChatEntry};

        let history = vec![ChatEntry::AssistantDone(vec![
            ChatBlock::Text("prose\n".to_string()),
            ChatBlock::Code { lang: "rust".to_string(), content: "fn main() {}\n".to_string() },
        ])];
        let lines = chat_entries_to_lines(&history);

        // lines[0] = "prose" — no background
        assert_eq!(lines[0].style, Style::default());

        // lines[1] = header " ┌─ rust " — Yellow foreground
        let header_style = Style::default().fg(Color::Yellow);
        assert_eq!(lines[1].style, header_style, "header line must be Yellow");

        // lines[2] = code body — dark bg + white fg
        let expected_style = Style::default().bg(Color::Rgb(30, 30, 30)).fg(Color::White);
        assert_eq!(lines[2].style, expected_style, "code line must have dark background + white fg");

        // lines[3] = footer " └────────" — Yellow foreground
        assert_eq!(lines[3].style, header_style, "footer line must be Yellow");
    }

    /// User entries render with a "[You]" header in Cyan bold followed by the text.
    #[test]
    fn user_entry_renders_with_you_prefix() {
        use crate::tui::ChatEntry;

        let history = vec![ChatEntry::User("hello".to_string())];
        let lines = chat_entries_to_lines(&history);

        // First line should contain "[You]" in Cyan bold
        let you_span = &lines[0].spans[0];
        assert_eq!(you_span.content, "[You]");
        assert_eq!(you_span.style.fg, Some(Color::Cyan));
        assert!(you_span.style.add_modifier.contains(Modifier::BOLD));

        // Second line should be the user text
        assert_eq!(lines[1].spans[0].content, "hello");
    }
}
