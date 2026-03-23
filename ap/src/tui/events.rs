//! `tui/events.rs` — Keyboard event handling (flat, always-active dispatch).
//!
//! Translates raw crossterm `KeyEvent`s into high-level [`Action`]s that
//! the [`TuiApp`] event loop can act on.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::TuiApp;

/// High-level action produced by a key event.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// No action required (re-render will happen regardless).
    None,
    /// The user submitted input — the string is the text to send to the agent.
    Submit(String),
    /// The user requested a quit.
    Quit,
    /// The user requested cancellation of the current in-progress turn.
    Cancel,
}

/// Translate a single key event into an [`Action`], mutating `app` for
/// immediate buffer or scroll changes.
///
/// All keys are always-active (no modal dispatch). The `is_waiting` guard
/// lives here: Enter is ignored when a turn is in progress. `handle_submit`
/// does **not** check `is_waiting` itself, keeping it directly callable from
/// tests.
pub fn handle_key_event(key: KeyEvent, app: &mut TuiApp) -> Action {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        // Ctrl+C → Cancel if waiting, Quit if idle
        KeyCode::Char('c') if ctrl => {
            if app.is_waiting {
                Action::Cancel
            } else {
                Action::Quit
            }
        }

        // Enter → Submit if buffer non-empty and not waiting, else None
        KeyCode::Enter => {
            if !app.is_waiting && !app.input_buffer.is_empty() {
                Action::Submit(app.input_buffer.drain(..).collect())
            } else {
                Action::None
            }
        }

        // Up arrow → scroll up by 3, unpin
        KeyCode::Up => {
            app.scroll_pinned = false;
            app.scroll_offset = app.scroll_offset.saturating_sub(3);
            Action::None
        }

        // Down arrow → scroll down by 3, unpin
        KeyCode::Down => {
            app.scroll_pinned = false;
            app.scroll_offset = app.scroll_offset.saturating_add(3);
            Action::None
        }

        // Backspace → remove last char from buffer
        KeyCode::Backspace => {
            app.input_buffer.pop();
            Action::None
        }

        // Any other character → append to buffer
        KeyCode::Char(c) => {
            app.input_buffer.push(c);
            Action::None
        }

        _ => Action::None,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn make_app() -> TuiApp {
        TuiApp::headless()
    }

    // ── 1. Enter with non-empty buffer submits ────────────────────────────────

    #[test]
    fn enter_with_non_empty_buffer_submits() {
        let mut app = make_app();
        app.input_buffer = "hello".to_string();
        app.is_waiting = false;
        let action = handle_key_event(key(KeyCode::Enter), &mut app);
        assert_eq!(action, Action::Submit("hello".to_string()));
    }

    // ── 2. Enter clears the buffer ────────────────────────────────────────────

    #[test]
    fn enter_clears_the_buffer() {
        let mut app = make_app();
        app.input_buffer = "hello".to_string();
        app.is_waiting = false;
        handle_key_event(key(KeyCode::Enter), &mut app);
        assert!(app.input_buffer.is_empty(), "buffer should be cleared after submit");
    }

    // ── 3. Enter with empty buffer returns None ───────────────────────────────

    #[test]
    fn enter_with_empty_buffer_returns_none() {
        let mut app = make_app();
        app.input_buffer = String::new();
        app.is_waiting = false;
        let action = handle_key_event(key(KeyCode::Enter), &mut app);
        assert_eq!(action, Action::None);
    }

    // ── 4. Enter when waiting returns None ───────────────────────────────────

    #[test]
    fn enter_when_waiting_returns_none() {
        let mut app = make_app();
        app.input_buffer = "hello".to_string();
        app.is_waiting = true;
        let action = handle_key_event(key(KeyCode::Enter), &mut app);
        assert_eq!(action, Action::None);
        // Buffer must NOT be drained
        assert_eq!(app.input_buffer, "hello", "buffer must not be drained when waiting");
    }

    // ── 5. Char appended to buffer ────────────────────────────────────────────

    #[test]
    fn char_appended_to_buffer() {
        let mut app = make_app();
        handle_key_event(key(KeyCode::Char('x')), &mut app);
        assert_eq!(app.input_buffer, "x");
    }

    // ── 6. Multiple chars build the buffer ───────────────────────────────────

    #[test]
    fn multiple_chars_build_buffer() {
        let mut app = make_app();
        handle_key_event(key(KeyCode::Char('a')), &mut app);
        handle_key_event(key(KeyCode::Char('b')), &mut app);
        handle_key_event(key(KeyCode::Char('c')), &mut app);
        assert_eq!(app.input_buffer, "abc");
    }

    // ── 7. Backspace removes last char ───────────────────────────────────────

    #[test]
    fn backspace_removes_last_char() {
        let mut app = make_app();
        app.input_buffer = "abc".to_string();
        let action = handle_key_event(key(KeyCode::Backspace), &mut app);
        assert_eq!(action, Action::None);
        assert_eq!(app.input_buffer, "ab");
    }

    // ── 8. Backspace on empty buffer is noop ─────────────────────────────────

    #[test]
    fn backspace_on_empty_buffer_is_noop() {
        let mut app = make_app();
        // Should not panic
        let action = handle_key_event(key(KeyCode::Backspace), &mut app);
        assert_eq!(action, Action::None);
        assert!(app.input_buffer.is_empty());
    }

    // ── 9. Ctrl+C when idle returns Quit ─────────────────────────────────────

    #[test]
    fn ctrl_c_when_idle_returns_quit() {
        let mut app = make_app();
        app.is_waiting = false;
        let action = handle_key_event(ctrl(KeyCode::Char('c')), &mut app);
        assert_eq!(action, Action::Quit);
    }

    // ── 10. Ctrl+C when waiting returns Cancel ───────────────────────────────

    #[test]
    fn ctrl_c_when_waiting_returns_cancel() {
        let mut app = make_app();
        app.is_waiting = true;
        let action = handle_key_event(ctrl(KeyCode::Char('c')), &mut app);
        assert_eq!(action, Action::Cancel);
    }

    // ── 11. Up arrow decrements scroll offset ────────────────────────────────

    #[test]
    fn up_arrow_decrements_scroll_offset() {
        let mut app = make_app();
        app.scroll_offset = 9;
        handle_key_event(key(KeyCode::Up), &mut app);
        assert_eq!(app.scroll_offset, 6);
    }

    // ── 12. Up arrow unpins autoscroll ───────────────────────────────────────

    #[test]
    fn up_arrow_unpins_autoscroll() {
        let mut app = make_app();
        app.scroll_pinned = true;
        handle_key_event(key(KeyCode::Up), &mut app);
        assert!(!app.scroll_pinned);
    }

    // ── 13. Up arrow at zero clamps to zero ──────────────────────────────────

    #[test]
    fn up_arrow_at_zero_clamps_to_zero() {
        let mut app = make_app();
        app.scroll_offset = 1;
        handle_key_event(key(KeyCode::Up), &mut app);
        assert_eq!(app.scroll_offset, 0);
    }

    // ── 14. Down arrow increments scroll offset ──────────────────────────────

    #[test]
    fn down_arrow_increments_scroll_offset() {
        let mut app = make_app();
        app.scroll_offset = 6;
        handle_key_event(key(KeyCode::Down), &mut app);
        assert_eq!(app.scroll_offset, 9);
    }

    // ── 15. Down arrow unpins autoscroll ─────────────────────────────────────

    #[test]
    fn down_arrow_unpins_autoscroll() {
        let mut app = make_app();
        app.scroll_pinned = true;
        handle_key_event(key(KeyCode::Down), &mut app);
        assert!(!app.scroll_pinned);
    }
}
