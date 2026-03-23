# PROMPT.md — TUI Overhaul: Simple Claude Code / pi Style UI

## Vision

Replace the current multi-pane ratatui TUI (which has broken keybindings, a vim-modal input model, split tool panels, and a busy layout) with a clean, minimal terminal chat UI in the style of pi or Claude Code.

The new UI has three zones, top to bottom:
1. **Status bar** — one line: model name · session/turn count · token context usage. Nothing else.
2. **Chat area** — scrollable message history. User messages prefixed `You: `, assistant messages prefixed `ap: `, tool invocations shown as a single dim inline annotation `[tool: bash]`. No boxes, no borders, no frames.
3. **Input bar** — single-line text input at the very bottom. Enter = submit. No modal modes. No multi-line editor.

The result must feel like a normal terminal chat tool: launch → type → Enter → response streams in. That's it.

---

## Guiding Principles

- **Functional-first**: `TuiApp` holds immutable-friendly state. Pure helper functions for rendering (no side effects inside render). Mutation only in event handlers and `handle_ui_event`.
- **No modal modes**: remove `AppMode::Normal` / `AppMode::Insert`. The input bar is always active.
- **Minimal dependencies on layout**: one `Layout::vertical` split is enough. No horizontal splits. No tool panel widget.
- **All existing unit tests that test `handle_ui_event` / `parse_chat_blocks` / theme must continue to pass** — those test the data model which is not changing. Tests for the old modal key-handling (`normal_mode_i_enters_insert`, `insert_mode_esc_returns_normal`, etc.) will be replaced with new simpler tests.
- **Build stays clean throughout every step**: each step must compile with `cargo build` and pass `cargo test` before the next begins.

---

## Current State (what exists today)

```
src/tui/
  mod.rs      — TuiApp struct, event loop, handle_ui_event, parse_chat_blocks
  events.rs   — handle_key_event → Action; AppMode (Normal/Insert)
  ui.rs       — render() with 4-pane layout: status + [conversation | tools] + input
  theme.rs    — Theme / Rose Pine palette (keep as-is)
```

Key types that must be **preserved unchanged**:
- `ChatBlock`, `ChatEntry`, `parse_chat_blocks()` — data model is fine
- `ToolEntry` — keep the struct (still used for inline annotations)
- `TuiApp` fields that back `handle_ui_event` logic: `chat_history`, `tool_entries`, `input_buffer`, `scroll_offset`, `scroll_pinned`, `is_waiting`, `model_name`, `total_input_tokens`, `total_output_tokens`, `last_input_tokens`, `context_limit`, `conversation_messages`
- `Theme` — keep entirely

Key things to **remove or replace**:
- `AppMode` enum and all modal logic
- `ToolEntry::expanded`, `ToolEntry::selected` — selection/expand UX gone
- `TuiApp::selected_tool`, `TuiApp::show_help` — gone
- All four-pane layout code in `ui.rs`
- All modal key-binding tests

---

## Technical Requirements

### Types and Signatures

#### `src/tui/mod.rs` — updated `TuiApp`

```rust
pub struct TuiApp {
    // ── Rendering state ───────────────────────────────────────────────────
    pub theme: Theme,
    pub chat_history: Vec<ChatEntry>,
    pub tool_entries: Vec<ToolEntry>,   // kept for handle_ui_event compat
    pub input_buffer: String,           // single-line; newlines stripped on submit
    pub scroll_offset: usize,
    pub scroll_pinned: bool,
    pub model_name: String,

    // ── Counters ──────────────────────────────────────────────────────────
    pub conversation_messages: usize,
    pub is_waiting: bool,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub last_input_tokens: u32,
    pub context_limit: Option<u32>,

    // ── Internal channels / shared state (private) ────────────────────────
    ui_tx: mpsc::Sender<TurnEvent>,
    ui_rx: Option<mpsc::Receiver<TurnEvent>>,
    conv: Arc<tokio::sync::Mutex<Conversation>>,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<Middleware>,
}
```

**Removed from `TuiApp`**: `mode: AppMode`, `selected_tool: Option<usize>`, `show_help: bool`.

#### `src/tui/events.rs` — simplified `Action` + `handle_key_event`

```rust
#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Submit(String),   // trimmed, non-empty input
    Quit,
    ScrollUp,
    ScrollDown,
    ScrollToBottom,
}

/// Pure key → Action translation. Mutates only `app.input_buffer`.
pub fn handle_key_event(key: KeyEvent, app: &mut TuiApp) -> Action;
```

Key bindings (no modes):
| Key | Action |
|-----|--------|
| `Enter` | `Submit(input_buffer.drain())` — only if non-whitespace and not `is_waiting` |
| `Backspace` | Remove last char from `input_buffer`; `Action::None` |
| `Char(c)` | Append to `input_buffer`; `Action::None` |
| `Up` | `ScrollUp` |
| `Down` | `ScrollDown` |
| `Ctrl+C` | `Quit` |
| `Ctrl+L` | `ScrollToBottom` (re-pin) |
| Everything else | `Action::None` |

No `Esc`, no `i`, no `G`, no `[`/`]`, no `e`, no `/help`.

#### `src/tui/ui.rs` — new minimal `render()`

```rust
/// Render the full TUI: status bar (top) + chat area (middle) + input bar (bottom).
pub fn render(frame: &mut Frame, app: &TuiApp);

/// Build the list of ratatui Lines for the chat area from chat_history + tool_entries.
/// Pure function — no I/O, testable.
pub fn chat_lines<'a>(history: &'a [ChatEntry], tool_entries: &'a [ToolEntry], theme: &Theme) -> Vec<Line<'a>>;

/// Format the status bar string. Pure function — testable.
pub fn status_text(model: &str, turns: usize, last_input_tokens: u32, context_limit: Option<u32>) -> String;
```

Layout (three rows, no columns):
```
┌─────────────────────────────────────────┐
│ status bar  (Length: 1)                 │
├─────────────────────────────────────────┤
│                                         │
│  chat area  (Min: 1)                    │
│                                         │
├─────────────────────────────────────────┤
│ > input bar (Length: 3)                 │
└─────────────────────────────────────────┘
```

Chat area rendering rules:
- `ChatEntry::User(text)` → `Line` prefixed with `Span::styled("You: ", accent+bold)` + plain text spans (word-wrap via `Wrap { trim: false }`)
- `ChatEntry::AssistantStreaming(text)` → `Line`s prefixed with `Span::styled("ap: ", success+bold)` on the first line only; continuation lines indented 4 spaces
- `ChatEntry::AssistantDone(blocks)` → same prefix, code blocks rendered as indented plain text with a `  ╔ lang ╗` / `  ╚══════╝` header/footer in `code_border` colour
- `ToolEntry` with `result == None` → single dim line `  · tool: {name} …` (in `theme.muted`)
- `ToolEntry` with `result == Some(_)` → single dim line `  · tool: {name} ✓` or `✗` (in `theme.success`/`theme.error`)

Input bar rendering:
- 3 rows tall (1 border top + 1 content + 1 border bottom)
- Border title: `" > "` when idle, `" ⏳ "` when `is_waiting`
- Border style: `theme.border_insert` (iris) always — no mode switching
- Shows `app.input_buffer` content with a visible cursor (`frame.set_cursor_position`)

Status bar:
- Format: `" ap │ {model} │ turns: {n} │ ctx: {k:.1}k{/limit_k (pct%)} "`
- Background: `theme.status_bar_bg`, foreground: `theme.status_bar_fg`, bold

---

## Ordered Implementation Steps

Each step must independently compile (`cargo build` clean) and pass `cargo test` before proceeding to the next.

---

### Step 1 — Remove `AppMode`, simplify `TuiApp` fields

**File**: `src/tui/mod.rs`, `src/tui/events.rs`

**Changes**:
1. Delete the `AppMode` enum entirely from `mod.rs`.
2. Remove these fields from `TuiApp`:
   - `mode: AppMode`
   - `selected_tool: Option<usize>`
   - `show_help: bool`
3. Update `TuiApp::new()` constructor to not set those fields.
4. Update `TuiApp::headless()` and `TuiApp::headless_with_limit()` to not set those fields.
5. In `mod.rs` `handle_ui_event`: remove any reference to `self.selected_tool` in `ToolStart` handler (just push to `tool_entries`, no auto-select).
6. In `events.rs`: replace `Action` variants and `handle_key_event` body entirely with the new no-modal bindings (see Technical Requirements above). Keep `Action::None`, `Action::Submit`, `Action::Quit`. Add `Action::ScrollUp`, `Action::ScrollDown`, `Action::ScrollToBottom`.
7. Update `mod.rs` `event_loop` to handle the new `Action` variants: `ScrollUp` decrements `scroll_offset` by 3 and unpins, `ScrollDown` increments by 3 and unpins, `ScrollToBottom` sets `scroll_offset = usize::MAX` and re-pins.
8. Remove all modal-related unit tests from `events.rs` (the ones testing `AppMode::Normal`/`Insert` transitions). Add new tests:
   - `enter_submits_when_not_waiting` — `Enter` with non-empty buffer produces `Submit`
   - `enter_does_nothing_when_waiting` — `Enter` with `is_waiting = true` produces `None`
   - `enter_does_nothing_when_empty` — `Enter` with empty buffer produces `None`
   - `char_appends_to_buffer`
   - `backspace_removes_last_char`
   - `ctrl_c_quits`
   - `up_scrolls_up`
   - `down_scrolls_down`
   - `ctrl_l_scrolls_to_bottom`

**Compile check**: `cargo build && cargo test` — existing `handle_ui_event` tests in `mod.rs` must all pass. Some `events.rs` tests will be deleted and replaced.

---

### Step 2 — Rewrite `ui.rs`: new three-zone layout

**File**: `src/tui/ui.rs`

**Changes**:
1. Delete all existing render functions: `render_status_bar`, `render_main_area`, `render_conversation`, `render_tool_panel`, `render_input_box`, `render_help_overlay`, `centered_rect`, `input_box_height`, `chat_entries_to_lines`.
2. Keep `format_ctx_segment` (used in tests) — or inline it into `status_text`. If kept, make it `pub(crate)`.
3. Implement `pub fn status_text(model: &str, turns: usize, last_input_tokens: u32, context_limit: Option<u32>) -> String` — pure, no Frame access.
4. Implement `pub fn chat_lines<'a>(history: &'a [ChatEntry], tool_entries: &'a [ToolEntry], theme: &Theme) -> Vec<Line<'a>>`:
   - Iterates `history` producing styled `Line`s as described in Technical Requirements.
   - Inserts `ToolEntry` annotation lines in document order (tool entries are appended in arrival order alongside messages, so render them interleaved: after the last user message and before the assistant response that follows, in the order they were pushed). For simplicity: render all tool entries as a block between the last `User` entry and the first `AssistantDone`/`AssistantStreaming` entry that follows it. If no ordering information is available, render tool entries after all chat history as a trailing block.
   - Pure: accepts slices, returns `Vec<Line>`, no side effects.
5. Implement `pub fn render(frame: &mut Frame, app: &TuiApp)`:
   - Three-row `Layout::vertical` split: `[Length(1), Min(1), Length(3)]`.
   - Row 0: `Paragraph::new(status_text(...))` with status bar style.
   - Row 1: `Paragraph::new(Text::from(chat_lines(...)))` with `Wrap { trim: false }`, `.scroll((clamped_offset, 0))`. Clamped offset: `app.scroll_offset.min(total_lines.saturating_sub(visible_rows))` where `visible_rows = area.height as usize`.
   - Row 2: input bar — `Paragraph` with border, title `" > "` or `" ⏳ "`, always `theme.border_insert` border style. Cursor positioned at end of `input_buffer` within the input area.
6. Remove `input_box_height` (no longer needed — fixed 3-row input).
7. Update unit tests in `ui.rs`:
   - Replace `input_box_height_*` tests with tests for `status_text` and `chat_lines`.
   - Keep and update `format_ctx_segment` tests if the function is preserved.
   - Add `chat_lines_user_prefix` — user entry produces a line starting with a span containing `"You: "` in accent bold.
   - Add `chat_lines_assistant_prefix` — assistant done entry produces lines starting with `"ap: "` in success bold.
   - Add `chat_lines_tool_annotation` — a completed tool entry appears as a dim line with the tool name and `✓`.
   - Add `chat_lines_tool_running` — an in-progress tool entry appears with `…`.
   - Add `status_text_no_limit` — contains model name and ctx without `%`.
   - Add `status_text_with_limit` — contains `%` usage.

**Compile check**: `cargo build && cargo test`.

---

### Step 3 — Wire scroll actions into `event_loop` + clamp scroll in render

**File**: `src/tui/mod.rs`

**Changes**:
1. In `event_loop`, handle the new `Action` variants returned by `handle_key_event`:
   ```rust
   Action::ScrollUp => {
       self.scroll_pinned = false;
       self.scroll_offset = self.scroll_offset.saturating_sub(3);
   }
   Action::ScrollDown => {
       self.scroll_pinned = false;
       self.scroll_offset = self.scroll_offset.saturating_add(3);
   }
   Action::ScrollToBottom => {
       self.scroll_pinned = true;
       self.scroll_offset = usize::MAX;
   }
   ```
2. Verify `handle_ui_event` still sets `scroll_offset = usize::MAX` when `scroll_pinned` is true on `TextChunk`, `ToolStart`, `ToolComplete`, `TurnEnd`, `ContextSummarized`. No changes needed — this was already correct.
3. Add/keep tests in `mod.rs`:
   - All existing `handle_ui_event` tests must still pass.
   - Add `scroll_up_action_decrements_and_unpins` — simulate `Action::ScrollUp` side-effects.
   - Add `scroll_to_bottom_action_repins` — simulate `Action::ScrollToBottom`.

**Compile check**: `cargo build && cargo test`.

---

### Step 4 — Integration smoke test + acceptance verification

**File**: `src/tui/mod.rs` (add a doc-test or integration test), optionally `tests/tui_smoke.rs`

**Changes**:
1. Add a `#[test]` in `mod.rs` (or `tests/tui_smoke.rs`) called `headless_new_ui_state`:
   - Constructs `TuiApp::headless()`.
   - Asserts no `mode` field exists (compile-time — just assert the struct fields that do exist).
   - Asserts `input_buffer` is empty.
   - Asserts `scroll_pinned` is true.
   - Asserts `is_waiting` is false.
   - Asserts `chat_history` is empty.
   - Asserts `tool_entries` is empty.
2. Add a test `submit_clears_buffer_and_pushes_user_entry`:
   - Uses `TuiApp::headless()`.
   - Simulates `handle_key_event` with several `Char` events followed by `Enter`.
   - Asserts `Action::Submit(...)` is returned and `input_buffer` is empty.
3. Add a test `waiting_prevents_submit`:
   - Sets `app.is_waiting = true`.
   - Fires `Enter`.
   - Asserts `Action::None` returned, buffer unchanged.
4. Run `cargo test` — all tests pass, zero warnings relevant to new code.

**Compile check**: `cargo build && cargo test`.

---

### Step 5 — Final cleanup and build validation

**Changes**:
1. Remove any dead code warnings:
   - `ToolEntry::expanded` field: if unused in the new UI, either remove it or `#[allow(dead_code)]` with a comment `// reserved for future detail view`.
   - Remove `format_ctx_segment` from `ui.rs` if it was inlined into `status_text` (or keep it as a private helper and test it through `status_text`).
2. Ensure `src/tui/theme.rs` is unchanged.
3. Ensure `src/types.rs` is unchanged.
4. Ensure `src/turn.rs` is unchanged.
5. Ensure `src/main.rs` `run_tui` compiles: `TuiApp::new(...)` signature is unchanged — constructor takes the same 6 arguments.
6. Run `cargo build --release` — zero errors, zero warnings (or only pre-existing warnings unrelated to TUI).
7. Run `cargo test` — all tests pass.
8. Run `cargo clippy -- -D warnings` — zero new clippy errors introduced by the TUI overhaul.

---

## File-by-File Change Summary

| File | Action |
|------|--------|
| `src/tui/theme.rs` | **No changes** |
| `src/types.rs` | **No changes** |
| `src/turn.rs` | **No changes** |
| `src/main.rs` | **No changes** (TuiApp::new signature stays the same) |
| `src/tui/mod.rs` | Remove `AppMode`/`selected_tool`/`show_help`; update constructor; handle new scroll actions in event_loop; handle_ui_event unchanged |
| `src/tui/events.rs` | New `Action` variants; new no-modal `handle_key_event`; new tests |
| `src/tui/ui.rs` | Full rewrite: `render`, `chat_lines`, `status_text`; remove all old render functions |

---

## Acceptance Criteria

All of the following must be true before the task is complete:

- [ ] **AC1**: `cargo build` succeeds with zero errors.
- [ ] **AC2**: `cargo test` passes — all tests green, including all pre-existing `handle_ui_event` tests in `mod.rs` and theme tests in `ui.rs`.
- [ ] **AC3**: `AppMode` enum does not exist anywhere in the codebase (`grep -r AppMode src/` returns nothing).
- [ ] **AC4**: `TuiApp` has no `mode`, `selected_tool`, or `show_help` fields.
- [ ] **AC5**: `handle_key_event` in `events.rs` handles `Enter` as submit (not newline-insert), `Up`/`Down` as scroll, `Ctrl+C` as quit — verified by unit tests.
- [ ] **AC6**: `ui::render` uses a three-zone vertical layout (status + chat + input), no horizontal splits — verified by reading the source.
- [ ] **AC7**: `chat_lines` is a pure function with unit tests covering user prefix, assistant prefix, tool annotation (running and done).
- [ ] **AC8**: `status_text` is a pure function with unit tests covering the with-limit and without-limit cases.
- [ ] **AC9**: Input bar is always 3 rows tall (not dynamic). Verified: no `input_box_height` function exists.
- [ ] **AC10**: No `show_help` or help overlay rendering code exists (`grep -r show_help src/` and `grep -r render_help src/` return nothing).
- [ ] **AC11**: `cargo clippy -- -D warnings` produces zero new warnings from files modified in this task.

---

## Output

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project builds clean.
