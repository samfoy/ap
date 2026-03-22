//! `tui/events.rs` — Keyboard event handling and mode state machine.
//!
//! Translates raw crossterm `KeyEvent`s into high-level [`Action`]s that
//! the [`TuiApp`] event loop can act on.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::{AppMode, TuiApp};

/// High-level action produced by a key event.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// No action required (re-render will happen regardless).
    None,
    /// The user submitted input — the string is the text to send to the agent.
    Submit(String),
    /// The user requested a quit.
    Quit,
}

/// Translate a single key event into an [`Action`], mutating `app` for
/// immediate mode or scroll changes.
pub fn handle_key_event(key: KeyEvent, app: &mut TuiApp) -> Action {
    // Dismiss help overlay with any key that doesn't conflict, or Esc.
    if app.show_help {
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Action::Quit;
            }
            app.show_help = false;
        }
        return Action::None;
    }

    match app.mode {
        AppMode::Normal => match (key.code, key.modifiers) {
            // Quit
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => Action::Quit,
            // Enter insert mode
            (KeyCode::Char('i'), _) | (KeyCode::Enter, _) => {
                app.mode = AppMode::Insert;
                Action::None
            }
            // Scroll down
            (KeyCode::Char('j'), _) | (KeyCode::PageDown, _) => {
                app.scroll_offset = app.scroll_offset.saturating_add(3);
                Action::None
            }
            // Scroll up
            (KeyCode::Char('k'), _) | (KeyCode::PageUp, _) => {
                app.scroll_offset = app.scroll_offset.saturating_sub(3);
                Action::None
            }
            // Esc in normal mode: hide help (belt-and-suspenders)
            (KeyCode::Esc, _) => {
                app.show_help = false;
                Action::None
            }
            _ => Action::None,
        },

        AppMode::Insert => match (key.code, key.modifiers) {
            // Quit
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => Action::Quit,
            // Return to normal mode
            (KeyCode::Esc, _) => {
                app.mode = AppMode::Normal;
                Action::None
            }
            // Submit input
            (KeyCode::Enter, _) => {
                let input: String = app.input_buffer.drain(..).collect();
                Action::Submit(input)
            }
            // Delete last character
            (KeyCode::Backspace, _) => {
                app.input_buffer.pop();
                Action::None
            }
            // Append character
            (KeyCode::Char(c), _) => {
                app.input_buffer.push(c);
                Action::None
            }
            _ => Action::None,
        },
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

    #[test]
    fn normal_mode_i_enters_insert() {
        let mut app = make_app();
        assert_eq!(app.mode, AppMode::Normal);
        let action = handle_key_event(key(KeyCode::Char('i')), &mut app);
        assert_eq!(action, Action::None);
        assert_eq!(app.mode, AppMode::Insert);
    }

    #[test]
    fn insert_mode_esc_returns_normal() {
        let mut app = make_app();
        app.mode = AppMode::Insert;
        let action = handle_key_event(key(KeyCode::Esc), &mut app);
        assert_eq!(action, Action::None);
        assert_eq!(app.mode, AppMode::Normal);
    }

    #[test]
    fn insert_mode_enter_submits_buffer() {
        let mut app = make_app();
        app.mode = AppMode::Insert;
        app.input_buffer = "hello world".to_string();
        let action = handle_key_event(key(KeyCode::Enter), &mut app);
        assert_eq!(action, Action::Submit("hello world".to_string()));
        assert!(app.input_buffer.is_empty(), "buffer should be cleared after submit");
    }

    #[test]
    fn ctrl_c_quits_in_normal_mode() {
        let mut app = make_app();
        let action = handle_key_event(ctrl(KeyCode::Char('c')), &mut app);
        assert_eq!(action, Action::Quit);
    }

    #[test]
    fn ctrl_c_quits_in_insert_mode() {
        let mut app = make_app();
        app.mode = AppMode::Insert;
        let action = handle_key_event(ctrl(KeyCode::Char('c')), &mut app);
        assert_eq!(action, Action::Quit);
    }

    #[test]
    fn normal_mode_scroll_j_increments_offset() {
        let mut app = make_app();
        handle_key_event(key(KeyCode::Char('j')), &mut app);
        assert_eq!(app.scroll_offset, 3);
    }

    #[test]
    fn normal_mode_scroll_k_decrements_offset() {
        let mut app = make_app();
        app.scroll_offset = 6;
        handle_key_event(key(KeyCode::Char('k')), &mut app);
        assert_eq!(app.scroll_offset, 3);
    }

    #[test]
    fn insert_mode_backspace_removes_last_char() {
        let mut app = make_app();
        app.mode = AppMode::Insert;
        app.input_buffer = "abc".to_string();
        handle_key_event(key(KeyCode::Backspace), &mut app);
        assert_eq!(app.input_buffer, "ab");
    }

    #[test]
    fn help_overlay_dismissed_by_esc() {
        let mut app = make_app();
        app.show_help = true;
        let action = handle_key_event(key(KeyCode::Esc), &mut app);
        assert_eq!(action, Action::None);
        assert!(!app.show_help);
    }
}
