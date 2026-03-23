//! `tui/theme.rs` — Color theme for the TUI.
//!
//! [`Theme`] holds every color used by [`crate::tui::ui`] as named semantic
//! slots.  [`Theme::rose_pine`] returns the Rose Pineé palette; that is also
//! the [`Default`] implementation.

use ratatui::style::Color;

/// All colors used by the TUI renderer, as semantic slots.
///
/// Create a custom theme by constructing this struct manually.  The
/// [`Default`] implementation is Rose Pine.
#[derive(Debug, Clone)]
pub struct Theme {
    // ── Status bar ────────────────────────────────────────────────────────
    pub status_bar_bg: Color,
    pub status_bar_fg: Color,
    // ── Semantic / accent ─────────────────────────────────────────────────
    pub accent: Color,
    pub success: Color,
    pub error: Color,
    pub warning: Color,
    pub muted: Color,
    pub dim: Color,
    pub text_color: Color,
    // ── Message backgrounds ───────────────────────────────────────────────
    pub user_msg_bg: Color,
    pub tool_ok_bg: Color,
    pub tool_err_bg: Color,
    pub selected_bg: Color,
    // ── Code blocks ───────────────────────────────────────────────────────
    pub code_bg: Color,
    pub code_fg: Color,
    pub code_border: Color,
    // ── Borders ───────────────────────────────────────────────────────────
    pub border_normal: Color,
    pub border_insert: Color,
    // ── Markdown ─────────────────────────────────────────────────────────
    pub md_heading: Color,
    // ── Syntax highlighting ───────────────────────────────────────────────
    pub syntax_keyword: Color,
    pub syntax_function: Color,
    pub syntax_string: Color,
    pub syntax_type: Color,
    pub syntax_comment: Color,
}

impl Theme {
    /// Rosé Pine palette — <https://rosepinetheme.com/palette/>.
    pub fn rose_pine() -> Self {
        // ── Raw palette ──────────────────────────────────────────────────
        let base    = Color::Rgb(25,  23,  36);
        let surface = Color::Rgb(31,  29,  46);
        let overlay = Color::Rgb(38,  35,  58);
        let muted   = Color::Rgb(110, 106, 134);
        let subtle  = Color::Rgb(144, 140, 170);
        let text    = Color::Rgb(224, 222, 244);
        let love    = Color::Rgb(235, 111, 146);
        let gold    = Color::Rgb(246, 193, 119);
        let rose    = Color::Rgb(235, 188, 186);
        let pine    = Color::Rgb(49,  116, 143);
        let foam    = Color::Rgb(156, 207, 216);
        let iris    = Color::Rgb(196, 167, 231);
        let hl_med  = Color::Rgb(64,  61,  82);

        // Suppress unused-variable warnings for palette entries that are only
        // here for completeness (base, rose in syntax slots etc.).
        let _ = base;

        Self {
            status_bar_bg:   overlay,
            status_bar_fg:   text,
            accent:          iris,
            success:         foam,
            error:           love,
            warning:         gold,
            muted,
            dim:             subtle,
            text_color:      text,
            user_msg_bg:     surface,
            tool_ok_bg:      Color::Rgb(30, 36, 48),
            tool_err_bg:     Color::Rgb(42, 30, 40),
            selected_bg:     overlay,
            code_bg:         surface,
            code_fg:         foam,
            code_border:     hl_med,
            border_normal:   hl_med,
            border_insert:   iris,
            md_heading:      rose,
            syntax_keyword:  pine,
            syntax_function: rose,
            syntax_string:   gold,
            syntax_type:     foam,
            syntax_comment:  muted,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::rose_pine()
    }
}
