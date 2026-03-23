//! `tui/ui.rs` — Ratatui layout and rendering.
//!
//! Pure rendering function: takes a [`Frame`] and [`TuiApp`] reference, draws
//! the three-pane layout (status bar, chat area, input line).

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::{Theme, TuiApp};

// ─── Pricing constants (Claude 3.5 Sonnet) ────────────────────────────────────

const COST_PER_M_INPUT: f64 = 3.00;
const COST_PER_M_OUTPUT: f64 = 15.00;

/// Render the full TUI into `frame`.
pub fn render(frame: &mut Frame, app: &TuiApp) {
    let outer = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .split(frame.area());

    render_status_bar(frame, app, outer[0]);
    render_chat_area(frame, app, outer[1]);
    render_input_line(frame, app, outer[2]);
}

// ─── Status bar ───────────────────────────────────────────────────────────────

/// Format the `ctx:` segment of the status bar.
///
/// Always shows `ctx: XX.Xk`. When `context_limit` is `Some`, also shows
/// `/YYYk (ZZ%)`.
pub(crate) fn format_ctx_segment(last_input_tokens: u32, context_limit: Option<u32>) -> String {
    let current_k = last_input_tokens as f32 / 1_000.0;
    context_limit.map_or_else(
        || format!("ctx: {current_k:.1}k"),
        |limit| {
            let limit_k = limit as f32 / 1_000.0;
            let pct = (last_input_tokens as f32 / limit as f32 * 100.0).round() as u32;
            format!("ctx: {current_k:.1}k/{limit_k:.0}k ({pct}%)")
        },
    )
}

fn render_status_bar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let input_k = app.total_input_tokens as f64 / 1_000.0;
    let output_k = app.total_output_tokens as f64 / 1_000.0;
    let cost = (app.total_input_tokens as f64 / 1_000_000.0) * COST_PER_M_INPUT
        + (app.total_output_tokens as f64 / 1_000_000.0) * COST_PER_M_OUTPUT;
    let ctx_segment = format_ctx_segment(app.last_input_tokens, app.context_limit);
    let text = format!(
        " ap │ {} │ Msgs: {} │ Tokens: ↑{:.1}k ↓{:.1}k │ Cost: ${:.4} │ {}",
        app.model_name,
        app.conversation_messages,
        input_k,
        output_k,
        cost,
        ctx_segment,
    );
    let status = Paragraph::new(text).style(
        Style::default()
            .bg(app.theme.status_bar_bg)
            .fg(app.theme.status_bar_fg)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(status, area);
}

// ─── Chat area ────────────────────────────────────────────────────────────────

/// Convert a slice of [`ChatEntry`]s into ratatui [`Line`]s for rendering.
///
/// User entries are prefixed inline with `You: ` in `theme.accent` bold.
/// Code blocks are rendered with `theme.code_bg`/`theme.code_fg` and
/// `theme.code_border` header/footer lines.  Tool calls are rendered with
/// status icons (⟳/✓/✗) colored by status.
/// Used by [`render_chat_area`] and unit tests.
pub fn chat_entries_to_lines<'a>(
    history: &'a [crate::tui::ChatEntry],
    theme: &Theme,
) -> Vec<Line<'a>> {
    use crate::tui::{ChatBlock, ChatEntry, ToolStatus};

    let mut lines: Vec<Line> = Vec::new();

    for entry in history {
        match entry {
            ChatEntry::User(text) => {
                // "You: " prefix inline on first line, accent bold
                let prefix_span = Span::styled(
                    "You: ",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                );
                let mut text_lines = text.lines();
                // First line: "You: " + content
                let first_content = text_lines.next().unwrap_or("");
                lines.push(Line::from(vec![
                    prefix_span,
                    Span::raw(first_content.to_string()),
                ]));
                // Remaining lines (if any) indented
                for line in text_lines {
                    lines.push(Line::from(line.to_string()));
                }
                lines.push(Line::from(""));
            }
            ChatEntry::AssistantStreaming(text) => {
                // "ap: " prefix inline on first line, accent bold
                let prefix_span = Span::styled(
                    "ap: ",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                );
                let mut text_lines = text.lines();
                // First line: "ap: " + content
                let first_content = text_lines.next().unwrap_or("");
                lines.push(Line::from(vec![
                    prefix_span,
                    Span::raw(first_content.to_string()),
                ]));
                // Remaining lines (if any) without prefix
                for line in text_lines {
                    lines.push(Line::from(line.to_string()));
                }
            }
            ChatEntry::AssistantDone(blocks) => {
                let mut first_text_line = true;
                for block in blocks {
                    match block {
                        ChatBlock::Text(text) => {
                            let mut text_lines = text.lines();
                            if first_text_line {
                                // "ap: " prefix on the very first text line of the response
                                let prefix_span = Span::styled(
                                    "ap: ",
                                    Style::default()
                                        .fg(theme.accent)
                                        .add_modifier(Modifier::BOLD),
                                );
                                let first_content = text_lines.next().unwrap_or("");
                                lines.push(Line::from(vec![
                                    prefix_span,
                                    Span::raw(first_content.to_string()),
                                ]));
                                first_text_line = false;
                            }
                            for line in text_lines {
                                lines.push(Line::from(line.to_string()));
                            }
                        }
                        ChatBlock::Code { lang, content } => {
                            let border_style = Style::default().fg(theme.code_border);
                            // Header line
                            lines.push(Line::styled(format!(" ┌─ {lang} "), border_style));
                            // Body lines
                            let code_style = Style::default()
                                .bg(theme.code_bg)
                                .fg(theme.code_fg);
                            for line in content.lines() {
                                lines.push(Line::styled(line.to_string(), code_style));
                            }
                            // Footer line
                            lines.push(Line::styled(" └────────", border_style));
                        }
                    }
                }
                lines.push(Line::from(""));
            }
            ChatEntry::System(text) => {
                let style = Style::default().fg(theme.muted);
                for line in text.lines() {
                    lines.push(Line::styled(format!("  ◆ {line}"), style));
                }
                lines.push(Line::from(""));
            }
            ChatEntry::ToolCall { name, status, output_snippet } => {
                let (icon, icon_style) = match status {
                    ToolStatus::Running => (
                        "⟳",
                        Style::default().fg(theme.warning),
                    ),
                    ToolStatus::Done => (
                        "✓",
                        Style::default().fg(theme.success),
                    ),
                    ToolStatus::Error => (
                        "✗",
                        Style::default().fg(theme.error),
                    ),
                };
                // First line: "  {icon} " + name
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(icon, icon_style),
                    Span::raw(format!(" {name}")),
                ]));
                // Optional snippet lines (styled with error color for visibility)
                if let Some(snippet) = output_snippet {
                    for line in snippet.lines() {
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(line.to_string(), Style::default().fg(theme.error)),
                        ]));
                    }
                }
            }
        }
    }

    lines
}

fn render_chat_area(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let lines = chat_entries_to_lines(&app.chat_history, &app.theme);
    let total_lines = lines.len();

    // Inner height = area height minus 2 border rows
    let visible_lines = (area.height as usize).saturating_sub(2);

    // Clamp scroll_offset: usize::MAX is the "pinned to bottom" sentinel.
    // Convert to a real row offset that ratatui can use (max u16 = 65535 would
    // scroll past all content and show a blank pane).
    let effective_offset = if app.scroll_offset == usize::MAX {
        total_lines.saturating_sub(visible_lines)
    } else {
        app.scroll_offset.min(total_lines.saturating_sub(visible_lines))
    };

    use ratatui::text::Text;
    let para = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("Conversation"))
        .wrap(Wrap { trim: false })
        .scroll((effective_offset as u16, 0));
    frame.render_widget(para, area);
}

// ─── Input line ───────────────────────────────────────────────────────────────

fn render_input_line(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let border_style = Style::default().fg(app.theme.border_normal);
    let para = Paragraph::new(app.input_buffer.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" > ")
            .border_style(border_style),
    );
    frame.render_widget(para, area);

    // Set cursor at end of input buffer, inside the border
    let x = area.x + 1 + app.input_buffer.len() as u16;
    let y = area.y + 1;
    // Guard: stay within area bounds
    if x < area.x + area.width.saturating_sub(1) && y < area.y + area.height.saturating_sub(1) {
        frame.set_cursor_position((x, y));
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> TuiApp {
        TuiApp::headless()
    }

    // ─── ToolCall rendering tests ─────────────────────────────────────────────

    /// Running tool call renders ⟳ icon styled with warning color.
    #[test]
    fn toolcall_running_renders_spinning_icon() {
        use crate::tui::{ChatEntry, ToolStatus};

        let theme = Theme::default();
        let history = vec![ChatEntry::ToolCall {
            name: "bash".to_string(),
            status: ToolStatus::Running,
            output_snippet: None,
        }];
        let lines = chat_entries_to_lines(&history, &theme);

        assert_eq!(lines.len(), 1, "Running ToolCall: exactly 1 line");
        let icon_span = lines[0].spans.iter().find(|s| s.content.contains('⟳'));
        assert!(icon_span.is_some(), "Running ToolCall: must contain ⟳ icon");
        let icon_span = icon_span.unwrap();
        assert_eq!(
            icon_span.style.fg,
            Some(theme.warning),
            "Running icon must use theme.warning color"
        );
    }

    /// Done tool call renders ✓ icon styled with success color.
    #[test]
    fn toolcall_done_renders_check_icon() {
        use crate::tui::{ChatEntry, ToolStatus};

        let theme = Theme::default();
        let history = vec![ChatEntry::ToolCall {
            name: "bash".to_string(),
            status: ToolStatus::Done,
            output_snippet: None,
        }];
        let lines = chat_entries_to_lines(&history, &theme);

        assert_eq!(lines.len(), 1, "Done ToolCall: exactly 1 line");
        let icon_span = lines[0].spans.iter().find(|s| s.content.contains('✓'));
        assert!(icon_span.is_some(), "Done ToolCall: must contain ✓ icon");
        let icon_span = icon_span.unwrap();
        assert_eq!(
            icon_span.style.fg,
            Some(theme.success),
            "Done icon must use theme.success color"
        );
    }

    /// Error tool call renders ✗ icon and snippet lines.
    #[test]
    fn toolcall_error_renders_x_icon_and_snippet() {
        use crate::tui::{ChatEntry, ToolStatus};

        let theme = Theme::default();
        let history = vec![ChatEntry::ToolCall {
            name: "bash".to_string(),
            status: ToolStatus::Error,
            output_snippet: Some("err output".to_string()),
        }];
        let lines = chat_entries_to_lines(&history, &theme);

        // At least 2 lines: header + 1 snippet line
        assert!(lines.len() >= 2, "Error ToolCall with snippet: at least 2 lines, got {}", lines.len());

        // First line must contain ✗
        let icon_span = lines[0].spans.iter().find(|s| s.content.contains('✗'));
        assert!(icon_span.is_some(), "Error ToolCall: first line must contain ✗ icon");

        // Second line must contain "err output"
        let snippet_text: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            snippet_text.contains("err output"),
            "Error ToolCall: second line must contain snippet text, got: {snippet_text:?}"
        );
    }

    /// Error tool call with no snippet renders exactly 1 line.
    #[test]
    fn toolcall_error_no_snippet_renders_one_line() {
        use crate::tui::{ChatEntry, ToolStatus};

        let theme = Theme::default();
        let history = vec![ChatEntry::ToolCall {
            name: "bash".to_string(),
            status: ToolStatus::Error,
            output_snippet: None,
        }];
        let lines = chat_entries_to_lines(&history, &theme);

        assert_eq!(lines.len(), 1, "Error ToolCall with no snippet: exactly 1 line");
        let icon_span = lines[0].spans.iter().find(|s| s.content.contains('✗'));
        assert!(icon_span.is_some(), "Error ToolCall: must contain ✗ icon");
    }

    // ─── Code block rendering tests ───────────────────────────────────────────

    /// Code block header/footer lines use `theme.code_border`; body lines use
    /// `theme.code_bg` / `theme.code_fg`.
    #[test]
    fn code_block_lines_have_dark_bg_style() {
        use crate::tui::{ChatBlock, ChatEntry};

        let theme = Theme::default();
        let history = vec![ChatEntry::AssistantDone(vec![
            ChatBlock::Text("prose\n".to_string()),
            ChatBlock::Code { lang: "rust".to_string(), content: "fn main() {}\n".to_string() },
        ])];
        let lines = chat_entries_to_lines(&history, &theme);

        // lines[0] = "prose" — no background
        assert_eq!(lines[0].style, Style::default());

        // lines[1] = header " ┌─ rust " — code_border foreground
        let header_style = Style::default().fg(theme.code_border);
        assert_eq!(lines[1].style, header_style, "header line must use theme.code_border");

        // lines[2] = code body — code_bg + code_fg
        let expected_style = Style::default().bg(theme.code_bg).fg(theme.code_fg);
        assert_eq!(lines[2].style, expected_style, "code line must use theme.code_bg + code_fg");

        // lines[3] = footer " └────────" — code_border foreground
        assert_eq!(lines[3].style, header_style, "footer line must use theme.code_border");
    }

    // ─── User prefix tests ────────────────────────────────────────────────────

    /// User entries render with `You: ` inline prefix in accent bold (not `[You]` header).
    #[test]
    fn user_entry_renders_with_you_prefix() {
        use crate::tui::ChatEntry;

        let theme = Theme::default();
        let history = vec![ChatEntry::User("hello".to_string())];
        let lines = chat_entries_to_lines(&history, &theme);

        // First line: spans[0] = "You: " styled, spans[1] = "hello"
        assert!(
            lines[0].spans.len() >= 2,
            "User entry first line must have at least 2 spans (prefix + content)"
        );
        let prefix_span = &lines[0].spans[0];
        assert_eq!(prefix_span.content, "You: ", "prefix must be 'You: '");
        assert_eq!(prefix_span.style.fg, Some(theme.accent), "prefix must use theme.accent");
        assert!(
            prefix_span.style.add_modifier.contains(Modifier::BOLD),
            "prefix must be BOLD"
        );

        // Second span on same line should be the user text
        let content_span = &lines[0].spans[1];
        assert_eq!(content_span.content, "hello", "content must be on same line as prefix");
    }

    // ─── Assistant prefix tests ───────────────────────────────────────────────

    /// AssistantStreaming entries render with `ap: ` inline prefix in accent bold.
    #[test]
    fn assistant_streaming_renders_with_ap_prefix() {
        use crate::tui::ChatEntry;

        let theme = Theme::default();
        let history = vec![ChatEntry::AssistantStreaming("hello world".to_string())];
        let lines = chat_entries_to_lines(&history, &theme);

        // First line must have at least 2 spans: "ap: " prefix + content
        assert!(
            lines[0].spans.len() >= 2,
            "AssistantStreaming first line must have at least 2 spans (prefix + content)"
        );
        let prefix_span = &lines[0].spans[0];
        assert_eq!(prefix_span.content, "ap: ", "prefix must be 'ap: '");
        assert_eq!(prefix_span.style.fg, Some(theme.accent), "prefix must use theme.accent");
        assert!(
            prefix_span.style.add_modifier.contains(Modifier::BOLD),
            "prefix must be BOLD"
        );
    }

    /// AssistantDone entries render with `ap: ` inline prefix on the first text line.
    #[test]
    fn assistant_done_renders_with_ap_prefix() {
        use crate::tui::{ChatBlock, ChatEntry};

        let theme = Theme::default();
        let history = vec![ChatEntry::AssistantDone(vec![
            ChatBlock::Text("response text".to_string()),
        ])];
        let lines = chat_entries_to_lines(&history, &theme);

        // First line must have "ap: " prefix span
        assert!(
            lines[0].spans.len() >= 2,
            "AssistantDone first text line must have at least 2 spans (prefix + content)"
        );
        let prefix_span = &lines[0].spans[0];
        assert_eq!(prefix_span.content, "ap: ", "prefix must be 'ap: '");
        assert_eq!(prefix_span.style.fg, Some(theme.accent), "prefix must use theme.accent");
        assert!(
            prefix_span.style.add_modifier.contains(Modifier::BOLD),
            "prefix must be BOLD"
        );
    }

    // ─── Status bar ctx segment ───────────────────────────────────────────────

    #[test]
    fn status_bar_ctx_display_no_limit() {
        let s = format_ctx_segment(45200, None);
        assert!(s.contains("ctx: 45.2k"), "got: {s}");
        assert!(!s.contains('%'), "should not contain % when no limit: {s}");
    }

    #[test]
    fn status_bar_ctx_display_with_limit() {
        let s = format_ctx_segment(45200, Some(200000));
        assert!(s.contains("ctx: 45.2k/200k (23%)"), "got: {s}");
    }

    // ─── System entry rendering tests ────────────────────────────────────────

    /// System entry renders with muted color (`Color::Rgb(110, 106, 134)`).
    #[test]
    fn system_entry_renders_with_muted_style() {
        use crate::tui::ChatEntry;
        use ratatui::style::Color;

        let theme = Theme::default();
        let history = vec![ChatEntry::System("foo".to_string())];
        let lines = chat_entries_to_lines(&history, &theme);

        let muted_color = Color::Rgb(110, 106, 134);
        // Line::styled sets line.style, not span.style
        let has_muted = lines.iter().any(|line| {
            line.style.fg == Some(muted_color)
                || line.spans.iter().any(|span| span.style.fg == Some(muted_color))
        });
        assert!(has_muted, "System entry must have at least one line with theme.muted color");
    }

    /// System entry renders with `"  ◆ "` diamond prefix.
    #[test]
    fn system_entry_renders_with_diamond_prefix() {
        use crate::tui::ChatEntry;

        let theme = Theme::default();
        let history = vec![ChatEntry::System("hello".to_string())];
        let lines = chat_entries_to_lines(&history, &theme);

        let has_diamond = lines.iter().any(|line| {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            text.starts_with("  ◆ ")
        });
        assert!(has_diamond, "System entry must have a line starting with '  ◆ '");
    }

    /// System entry adds a trailing blank line.
    #[test]
    fn system_entry_adds_blank_line() {
        use crate::tui::ChatEntry;

        let theme = Theme::default();
        let history = vec![ChatEntry::System("msg".to_string())];
        let lines = chat_entries_to_lines(&history, &theme);

        assert!(!lines.is_empty(), "Must have at least 1 line");
        let last = lines.last().unwrap();
        let text: String = last.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.is_empty(),
            "Last line must be empty (trailing blank), got: {text:?}"
        );
    }

    // ─── Theme tests ──────────────────────────────────────────────────────────

    #[test]
    fn default_theme_is_rose_pine() {
        use ratatui::style::Color;
        let _ = make_app(); // silence unused import warning
        let theme = Theme::default();
        // iris = Rgb(196, 167, 231)
        assert_eq!(theme.accent, Color::Rgb(196, 167, 231));
        // overlay = Rgb(38, 35, 58)
        assert_eq!(theme.status_bar_bg, Color::Rgb(38, 35, 58));
        // foam = Rgb(156, 207, 216)
        assert_eq!(theme.success, Color::Rgb(156, 207, 216));
        // love = Rgb(235, 111, 146)
        assert_eq!(theme.error, Color::Rgb(235, 111, 146));
    }
}
