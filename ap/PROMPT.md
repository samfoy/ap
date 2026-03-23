# PROMPT.md — TUI Overhaul: Simple Claude Code / pi Style UI

## Vision

Replace the current multi-pane ratatui TUI with a clean, minimal terminal chat UI
in the style of pi or Claude Code. The new UI has exactly three zones, top-to-bottom:

1. **Status bar** — one line: `ap │ {model} │ turns: N │ ctx: X.Xk[/Yk (Z%)]`. Nothing else.
2. **Chat area** — scrollable message history. User messages prefixed `You: ` (accent
   bold). Assistant messages prefixed `ap: ` (success bold). Tool invocations shown as
   single dim inline annotations `  · tool: {name} …` / `  · tool: {name} ✓`. No
   boxes, no borders, no frames, no tool panel widget.
3. **Input bar** — single-line text input at the very bottom (3 rows: top-border +
   content + bottom-border). `Enter` = submit. No modal modes. No multi-line editor.

The result must feel like a normal terminal chat tool: launch → type → `Enter` →
response streams in inline. That is it.

---

## Guiding Principles

- **Functional-first**: `TuiApp` holds plain data fields. Rendering is done by pure
  helper functions (`chat_lines`, `status_text`) that accept slices/values and return
  ratatui types — no side effects inside render.
- **No modal modes**: remove `AppMode::Normal` / `AppMode::Insert`. The input bar is
  always active; characters always go into `input_buffer`.
- **Minimal layout**: one `Layout::vertical([Length(1), Min(1), Length(3)])`. No
  horizontal splits. No tool-panel column.
- **All existing `handle_ui_event` tests must continue to pass** — the data model
  (`ChatEntry`, `ChatBlock`, `ToolEntry`, `parse_chat_blocks`, `handle_ui_event`,
  token accumulation, scroll-pin logic) is not changing.
- **Every step is independently compilable**: `cargo build && cargo test` must be
  green before the next step begins.

---

## Current State (what exists today)

```
src/tui/
  mod.rs    — TuiApp struct, run()/event_loop(), handle_submit(), handle_ui_event(),
              parse_chat_blocks(), headless()/headless_with_limit() test constructors
  events.rs — Action enum; handle_key_event(); AppMode (Normal / Insert) modal FSM
  ui.rs     — render() 4-pane layout: status | [conversation | tools] | input
              chat_entries_to_lines(), input_box_height(), format_ctx_segment(),
              render_help_overlay(), centered_rect()
  theme.rs  — Theme struct + Rose Pine palette  ← DO NOT TOUCH
```

Core types in `src/types.rs` (`Conversation`, `TurnEvent`, `Middleware`) —
**DO NOT TOUCH**.

`src/turn.rs`, `src/main.rs` — **DO NOT TOUCH** (TuiApp::new signature stays the
same; run_tui compiles unchanged).

---

## Types and Signatures That Must Be Preserved

### Keep unchanged in `src/tui/mod.rs`

```rust
// ── Data types (unchanged) ────────────────────────────────────────────────
pub enum ChatBlock { Text(String), Code { lang: String, content: String } }
pub enum ChatEntry { User(String), AssistantStreaming(String), AssistantDone(Vec<ChatBlock>) }
pub fn parse_chat_blocks(text: &str) -> Vec<ChatBlock>
pub struct ToolEntry { pub name: String, pub params: String,
                       pub result: Option<String>, pub is_error: bool,
                       pub expanded: bool }   // expanded: keep field, unused in new UI

// ── TuiApp public fields (subset — see Step 1 for full updated struct) ───
pub chat_history: Vec<ChatEntry>
pub tool_entries: Vec<ToolEntry>
pub input_buffer: String
pub scroll_offset: usize
pub scroll_pinned: bool
pub model_name: String
pub conversation_messages: usize
pub is_waiting: bool
pub total_input_tokens: u32
pub total_output_tokens: u32
pub last_input_tokens: u32
pub context_limit: Option<u32>
// Private: ui_tx, ui_rx, conv, provider, tools, middleware — unchanged

// ── Constructor signature (unchanged — main.rs must not be touched) ──────
pub fn new(
    conv: Arc<tokio::sync::Mutex<Conversation>>,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<Middleware>,
    model_name: String,
    context_limit: Option<u32>,
) -> Result<Self>

pub fn handle_ui_event(&mut self, event: TurnEvent)   // logic unchanged
```

### New / changed signatures

#### `src/tui/events.rs`

```rust
#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Submit(String),      // non-empty trimmed input; only when !is_waiting
    Quit,
    ScrollUp,
    ScrollDown,
    ScrollToBottom,
}

/// Pure key-event → Action translation.
/// Only side effect allowed: mutate app.input_buffer.
pub fn handle_key_event(key: KeyEvent, app: &mut TuiApp) -> Action;
```

Key-binding table (no modes):

| Key | Condition | Action / Side-effect |
|-----|-----------|----------------------|
| `Enter` | `!is_waiting && !input_buffer.trim().is_empty()` | `Submit(drain input_buffer)` |
| `Enter` | `is_waiting` OR `input_buffer.trim().is_empty()` | `None` |
| `Backspace` | — | pop last char from `input_buffer`; `None` |
| `Char(c)` | — | push `c` to `input_buffer`; `None` |
| `Up` / `KeyCode::Up` | — | `ScrollUp` |
| `Down` / `KeyCode::Down` | — | `ScrollDown` |
| `Ctrl+C` | — | `Quit` |
| `Ctrl+L` | — | `ScrollToBottom` |
| anything else | — | `None` |

No `Esc`, no `i`, no `G`, no `j`/`k`, no `[`/`]`, no `e`, no `/help`.

#### `src/tui/ui.rs`

```rust
/// Format the status bar text. Pure — no Frame access.
/// Format: " ap │ {model} │ turns: {turns} │ {ctx_segment} "
pub fn status_text(
    model: &str,
    turns: usize,
    last_input_tokens: u32,
    context_limit: Option<u32>,
) -> String;

/// Build ratatui Lines for the chat area. Pure — testable with no terminal.
///
/// Rendering rules:
///   ChatEntry::User(text)
///     → Line: Span::styled("You: ", accent+BOLD) + Span::raw(text)
///       (one Line per line of text; only first line gets the prefix)
///   ChatEntry::AssistantStreaming(text)
///     → Line: Span::styled("ap: ", success+BOLD) + Span::raw(first_line)
///       continuation lines: Span::raw("    ") + Span::raw(line)
///   ChatEntry::AssistantDone(blocks)
///     → same prefix as Streaming; code blocks rendered as:
///         Span::styled("  ╔ {lang} ", code_border)  (header)
///         Span::styled("  {line}",   code_bg+code_fg) (body lines)
///         Span::styled("  ╚══════",  code_border)  (footer)
///       empty blank Line after each Done entry
///   ToolEntry { result: None, .. }
///     → Line: Span::styled("  · tool: {name} …", theme.muted)
///   ToolEntry { result: Some(_), is_error: false }
///     → Line: Span::styled("  · tool: {name} ✓", theme.success)
///   ToolEntry { result: Some(_), is_error: true }
///     → Line: Span::styled("  · tool: {name} ✗", theme.error)
///
/// Tool entries are rendered as a trailing block after all chat history.
pub fn chat_lines<'a>(
    history: &'a [ChatEntry],
    tool_entries: &'a [ToolEntry],
    theme: &Theme,
) -> Vec<Line<'a>>;

/// Render the full TUI into frame.
/// Layout: vertical [Length(1), Min(1), Length(3)]
/// - area[0]: status bar (Paragraph, status_bar_bg/fg, BOLD)
/// - area[1]: chat area (Paragraph + Wrap{trim:false} + scroll)
/// - area[2]: input bar (Paragraph with border, cursor)
pub fn render(frame: &mut Frame, app: &TuiApp);
```

---

## Ordered Implementation Steps

Each step **must compile and pass all tests** before the next begins.

---

### Step 1 — Strip modal state from `TuiApp` and rewrite `events.rs`

**Files**: `src/tui/mod.rs`, `src/tui/events.rs`

#### 1a. `src/tui/mod.rs` — remove modal fields

1. Delete the `AppMode` enum entirely.
2. Remove these three fields from `TuiApp`:
   - `pub mode: AppMode`
   - `pub selected_tool: Option<usize>`
   - `pub show_help: bool`
3. Update `TuiApp::new()` — remove initialisation of the three deleted fields.
4. Update `TuiApp::headless()` and `TuiApp::headless_with_limit()` — same.
5. In `handle_ui_event`, `ToolStart` arm: remove the `self.selected_tool = Some(...)` line (just push to `tool_entries`, no auto-select).
6. Any remaining reference to `self.mode`, `self.selected_tool`, `self.show_help` in `mod.rs` — delete.
7. In `event_loop`, update the match on `Action`:
   ```rust
   Action::Quit         => break,
   Action::Submit(text) => self.handle_submit(text).await,
   Action::ScrollUp     => {
       self.scroll_pinned = false;
       self.scroll_offset = self.scroll_offset.saturating_sub(3);
   }
   Action::ScrollDown   => {
       self.scroll_pinned = false;
       self.scroll_offset = self.scroll_offset.saturating_add(3);
   }
   Action::ScrollToBottom => {
       self.scroll_pinned = true;
       self.scroll_offset = usize::MAX;
   }
   Action::None => {}
   ```
8. Update or remove any unit test in `mod.rs` that references `AppMode`,
   `selected_tool`, or `show_help`. All `handle_ui_event` tests must still pass.

#### 1b. `src/tui/events.rs` — new no-modal key handler

1. Replace the `Action` enum with the new 6-variant version (add `ScrollUp`,
   `ScrollDown`, `ScrollToBottom`; keep `None`, `Submit`, `Quit`).
2. Replace `handle_key_event` entirely with the no-modal implementation described
   in the key-binding table above.
   - `Enter`: submit only if `!app.is_waiting` and trimmed buffer non-empty.
   - `Backspace`: `app.input_buffer.pop(); Action::None`.
   - `Char(c)`: `app.input_buffer.push(c); Action::None`.
   - `KeyCode::Up`: `Action::ScrollUp`.
   - `KeyCode::Down`: `Action::ScrollDown`.
   - `Ctrl+C`: `Action::Quit`.
   - `Ctrl+L`: `Action::ScrollToBottom`.
   - Everything else: `Action::None`.
3. Remove all old tests. Add these new tests:

```rust
#[test]
fn enter_submits_when_not_waiting()   // buffer="hello", is_waiting=false → Submit("hello"), buffer empty
#[test]
fn enter_does_nothing_when_waiting()  // buffer="hello", is_waiting=true  → None, buffer unchanged
#[test]
fn enter_does_nothing_when_empty()    // buffer="   ",   is_waiting=false → None
#[test]
fn char_appends_to_buffer()           // Char('x') → None, buffer="x"
#[test]
fn backspace_removes_last_char()      // buffer="ab", Backspace → None, buffer="a"
#[test]
fn backspace_on_empty_is_safe()       // buffer="",  Backspace → None, no panic
#[test]
fn ctrl_c_quits()                     // Ctrl+C → Quit
#[test]
fn up_returns_scroll_up()             // KeyCode::Up → ScrollUp
#[test]
fn down_returns_scroll_down()         // KeyCode::Down → ScrollDown
#[test]
fn ctrl_l_scrolls_to_bottom()         // Ctrl+L → ScrollToBottom
#[test]
fn unknown_key_returns_none()         // KeyCode::F(1) → None
```

**Compile check**: `cargo build && cargo test` — all green. The old modal tests in
`events.rs` are deleted and replaced. All `mod.rs` `handle_ui_event` tests pass.

---

### Step 2 — Rewrite `ui.rs`: three-zone layout

**File**: `src/tui/ui.rs`

1. **Delete** all existing functions:
   `render`, `render_status_bar`, `render_main_area`, `render_conversation`,
   `render_tool_panel`, `render_input_box`, `render_help_overlay`, `centered_rect`,
   `input_box_height`, `chat_entries_to_lines`.
   Preserve `format_ctx_segment` as a private helper `fn format_ctx_segment(...)`.
   (Its existing tests pin the format; keep them or inline into `status_text` tests.)

2. **Implement `status_text`**:
   ```rust
   pub fn status_text(
       model: &str,
       turns: usize,
       last_input_tokens: u32,
       context_limit: Option<u32>,
   ) -> String
   ```
   Format: `" ap │ {model} │ turns: {turns} │ {ctx_segment} "`
   where `{ctx_segment}` = `format_ctx_segment(last_input_tokens, context_limit)`.

3. **Implement `chat_lines`**:
   ```rust
   pub fn chat_lines<'a>(
       history: &'a [ChatEntry],
       tool_entries: &'a [ToolEntry],
       theme: &Theme,
   ) -> Vec<Line<'a>>
   ```
   Follow the rendering rules in the Types section above.
   Tool entries are appended as a trailing block after all chat history lines.
   Use `Line::from(vec![...spans...])` — not `Line::styled(...)` — so per-span
   styles work correctly inside `Paragraph::new(Text::from(lines))`.

4. **Implement `render`**:
   ```rust
   pub fn render(frame: &mut Frame, app: &TuiApp)
   ```
   - Split: `Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(3)]).split(frame.area())`.
   - `area[0]`: `Paragraph::new(status_text(...))` styled `status_bar_bg` / `status_bar_fg` / `BOLD`.
   - `area[1]`: `Paragraph::new(Text::from(chat_lines(...))).wrap(Wrap { trim: false }).scroll((clamped, 0))`.
     Clamped: `let visible = area[1].height as usize; let total = lines.len(); let max_offset = total.saturating_sub(visible); let clamped = app.scroll_offset.min(max_offset) as u16;`
   - `area[2]`: input bar.
     - Title: `if app.is_waiting { " ⏳ " } else { " > " }`.
     - Border style: always `theme.border_insert` (iris — no mode switching).
     - Content: `app.input_buffer.as_str()`.
     - Cursor: positioned at `(area[2].x + 1 + app.input_buffer.len() as u16, area[2].y + 1)`,
       clamped to within the widget bounds.

5. **Update tests** in `ui.rs` — delete old tests, add:
   ```rust
   #[test]
   fn status_text_no_limit()
   // status_text("my-model", 3, 45200, None)
   // contains "my-model", "turns: 3", "ctx: 45.2k", no '%'

   #[test]
   fn status_text_with_limit()
   // status_text("m", 1, 45200, Some(200000))
   // contains "ctx: 45.2k/200k (23%)"

   #[test]
   fn chat_lines_user_prefix()
   // history = [User("hello")], tool_entries = []
   // first Line's first Span content = "You: ", style has accent fg + BOLD

   #[test]
   fn chat_lines_assistant_streaming_prefix()
   // history = [AssistantStreaming("hi there")], tool_entries = []
   // first Line's first Span content = "ap: ", style has success fg + BOLD

   #[test]
   fn chat_lines_assistant_done_prefix()
   // history = [AssistantDone(vec![Text("answer\n")])], tool_entries = []
   // first Line's first Span = "ap: "

   #[test]
   fn chat_lines_tool_running()
   // history = [], tool_entries = [ToolEntry { name: "bash", result: None, .. }]
   // one Line whose content contains "bash" and "…", styled muted

   #[test]
   fn chat_lines_tool_done_ok()
   // tool_entries = [ToolEntry { name: "read", result: Some("x"), is_error: false, .. }]
   // Line content contains "read" and "✓"

   #[test]
   fn chat_lines_tool_done_err()
   // tool_entries = [ToolEntry { name: "bash", result: Some("err"), is_error: true, .. }]
   // Line content contains "bash" and "✗"

   #[test]
   fn chat_lines_empty()
   // history = [], tool_entries = [] → vec is empty

   #[test]
   fn format_ctx_segment_no_limit()   // keep existing test logic
   #[test]
   fn format_ctx_segment_with_limit() // keep existing test logic
   ```

**Compile check**: `cargo build && cargo test` — all green.

---

### Step 3 — Wire scroll actions + smoke-test the integration

**File**: `src/tui/mod.rs`

The `event_loop` scroll wiring was already done in Step 1. This step adds
integration-level tests that exercise the full action→state path.

1. Add tests in `mod.rs` (in the existing `#[cfg(test)]` block):

```rust
#[test]
fn scroll_up_action_decrements_and_unpins() {
    // app.scroll_offset = 10, scroll_pinned = true
    // simulate handle_key_event returning ScrollUp → apply side effects
    // (test the side-effect code directly, not via event_loop)
    let mut app = TuiApp::headless();
    app.scroll_offset = 10;
    app.scroll_pinned = true;
    // Apply the same logic as event_loop does for ScrollUp:
    app.scroll_pinned = false;
    app.scroll_offset = app.scroll_offset.saturating_sub(3);
    assert_eq!(app.scroll_offset, 7);
    assert!(!app.scroll_pinned);
}

#[test]
fn scroll_down_action_increments_and_unpins() {
    let mut app = TuiApp::headless();
    app.scroll_offset = 0;
    app.scroll_pinned = true;
    app.scroll_pinned = false;
    app.scroll_offset = app.scroll_offset.saturating_add(3);
    assert_eq!(app.scroll_offset, 3);
    assert!(!app.scroll_pinned);
}

#[test]
fn scroll_to_bottom_action_repins() {
    let mut app = TuiApp::headless();
    app.scroll_pinned = false;
    app.scroll_offset = 42;
    app.scroll_pinned = true;
    app.scroll_offset = usize::MAX;
    assert_eq!(app.scroll_offset, usize::MAX);
    assert!(app.scroll_pinned);
}

#[test]
fn headless_new_ui_state() {
    let app = TuiApp::headless();
    // Fields that exist (compile-time check via field access)
    assert!(app.input_buffer.is_empty());
    assert!(app.scroll_pinned);
    assert!(!app.is_waiting);
    assert!(app.chat_history.is_empty());
    assert!(app.tool_entries.is_empty());
    assert_eq!(app.scroll_offset, 0);
    assert_eq!(app.conversation_messages, 0);
    // Verify removed fields do NOT exist — this is enforced at compile time
    // (if AppMode or selected_tool existed, the headless() body would reference them
    //  and the compiler would catch any leftover reference)
}

#[test]
fn enter_key_produces_submit_and_clears_buffer() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = TuiApp::headless();
    // Type "hello" char by char
    for c in "hello".chars() {
        crate::tui::events::handle_key_event(
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), &mut app
        );
    }
    assert_eq!(app.input_buffer, "hello");
    // Press Enter
    let action = crate::tui::events::handle_key_event(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app
    );
    assert_eq!(action, crate::tui::events::Action::Submit("hello".to_string()));
    assert!(app.input_buffer.is_empty());
}

#[test]
fn waiting_prevents_submit() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = TuiApp::headless();
    app.is_waiting = true;
    app.input_buffer = "something".to_string();
    let action = crate::tui::events::handle_key_event(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app
    );
    assert_eq!(action, crate::tui::events::Action::None);
    assert_eq!(app.input_buffer, "something", "buffer must not be cleared when waiting");
}
```

2. Ensure all pre-existing `handle_ui_event` tests still pass (they should — no
   changes to that function in this step).

**Compile check**: `cargo build && cargo test` — all green.

---

### Step 4 — Final cleanup and build validation

1. **Dead code**: `ToolEntry::expanded` is kept but unused in the new UI. Add
   `#[allow(dead_code)]` on the field with a comment:
   `// reserved: future detail-expansion view`.
   Alternatively, remove it if no test references it — check with
   `grep -r 'expanded' src/`.

2. **Remove leftover imports** in `ui.rs`:
   - `use crate::tui::AppMode;` — gone (AppMode deleted).
   - Any import that the compiler warns about as unused.

3. **Remove leftover imports** in `events.rs` — same: `AppMode` import gone.

4. **Verify unchanged files**:
   ```sh
   git diff --name-only src/tui/theme.rs   # must be empty
   git diff --name-only src/types.rs       # must be empty
   git diff --name-only src/turn.rs        # must be empty
   git diff --name-only src/main.rs        # must be empty
   ```

5. **Full validation**:
   ```sh
   cargo build --release          # zero errors
   cargo test                     # all green
   cargo clippy -- -D warnings    # zero new warnings in modified files
   ```

6. **Acceptance grep checks** (run these and confirm output):
   ```sh
   grep -r 'AppMode'    src/   # → no matches
   grep -r 'show_help'  src/   # → no matches
   grep -r 'selected_tool' src/ # → no matches
   grep -r 'render_help' src/  # → no matches
   grep -r 'input_box_height' src/ # → no matches
   grep -r 'chat_entries_to_lines' src/ # → no matches
   ```

---

## File-by-File Change Summary

| File | Action |
|------|--------|
| `src/tui/theme.rs` | **No changes** |
| `src/types.rs` | **No changes** |
| `src/turn.rs` | **No changes** |
| `src/main.rs` | **No changes** (`TuiApp::new` signature unchanged) |
| `src/tui/mod.rs` | Remove `AppMode`/`selected_tool`/`show_help`; update constructor + headless constructors; wire new scroll Actions in `event_loop`; `handle_ui_event` logic unchanged; new integration tests |
| `src/tui/events.rs` | New `Action` variants; new no-modal `handle_key_event`; new tests replacing all old modal tests |
| `src/tui/ui.rs` | Full rewrite: `render`, `chat_lines`, `status_text`; remove all old render functions; new tests |

---

## Acceptance Criteria

- [ ] **AC1**: `cargo build` succeeds — zero errors.
- [ ] **AC2**: `cargo test` — all tests pass, including all pre-existing
  `handle_ui_event` tests in `mod.rs` (text chunk appending, tool start/complete,
  turn end, usage accumulation, scroll-pin behaviour, context summarized).
- [ ] **AC3**: `grep -rn 'AppMode' src/` → zero matches.
- [ ] **AC4**: `grep -rn 'show_help' src/` → zero matches.
- [ ] **AC5**: `grep -rn 'selected_tool' src/` → zero matches.
- [ ] **AC6**: `grep -rn 'input_box_height' src/` → zero matches.
- [ ] **AC7**: `grep -rn 'chat_entries_to_lines' src/` → zero matches.
- [ ] **AC8**: `grep -rn 'render_help' src/` → zero matches.
- [ ] **AC9**: `TuiApp` struct in `mod.rs` has no `mode`, `selected_tool`, or
  `show_help` fields.
- [ ] **AC10**: `handle_key_event` in `events.rs` maps `Enter` → `Submit` (not
  newline-insert), `Up`/`Down` → scroll actions, `Ctrl+C` → `Quit` — verified by
  unit tests `enter_submits_when_not_waiting`, `up_returns_scroll_up`,
  `down_returns_scroll_down`, `ctrl_c_quits`.
- [ ] **AC11**: `ui::render` uses a three-zone vertical-only layout (`Length(1)` +
  `Min(1)` + `Length(3)`). No horizontal splits. Verified by reading `render()`.
- [ ] **AC12**: `chat_lines` is a pure function with passing unit tests covering:
  user prefix (`"You: "` in accent+BOLD), assistant prefix (`"ap: "` in
  success+BOLD), running tool annotation (`…`), completed-ok annotation (`✓`),
  completed-error annotation (`✗`), empty input.
- [ ] **AC13**: `status_text` is a pure function with passing unit tests covering
  no-limit and with-limit (percentage) cases.
- [ ] **AC14**: Input bar is always exactly 3 rows (`Length(3)`) — not dynamic.
- [ ] **AC15**: `cargo clippy -- -D warnings` — zero new clippy errors or warnings
  in files modified by this task.

---

## Output

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project builds clean.
