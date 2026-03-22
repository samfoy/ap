# Context — richer-tui

## Summary

The `ap` codebase is a clean, functional-first Rust TUI agent. All 5 implementation steps touch exactly 3 files (`types.rs`, `turn.rs`, `tui/mod.rs`) plus the two TUI sub-modules (`tui/events.rs`, `tui/ui.rs`).

## Research Findings

### Integration Points

**Step 1 — Token Usage**
- `ap/src/types.rs:50`: Add `TurnEvent::Usage { input_tokens: u32, output_tokens: u32 }` variant after existing variants
- `ap/src/turn.rs:138`: Change `StreamEvent::TurnEnd { .. }` → `StreamEvent::TurnEnd { input_tokens, output_tokens, .. }` and push `TurnEvent::Usage { input_tokens, output_tokens }` before `apply_post_turn`
- `ap/src/tui/mod.rs`: Add `total_input_tokens: u32, total_output_tokens: u32` fields; add `Usage` arm to `handle_ui_event`; update `headless()`
- `ap/src/tui/ui.rs:render_status_bar`: Add token/cost display
- `ap/src/types.rs:tests`: Update `turn_event_variants_are_clonable` to include `Usage` (currently checks `cloned.len() == 5`)

**Step 2 — Multi-line Input**
- `ap/src/tui/events.rs`: In Insert mode, remap `KeyCode::Enter` → push `\n` to buffer; add `(KeyCode::Enter, m) if m.contains(KeyModifiers::CONTROL)` → drain buffer + `Action::Submit`
- `ap/src/tui/ui.rs`: Extract `fn input_box_height(app: &TuiApp) -> u16` (min 4, max 8); change outer layout from `Constraint::Length(3)` → `Constraint::Length(input_box_height(app))`
- Cursor position: multi-line needs `y = area.y + 1 + (cursor_row)` and `x` wraps — simplest is count `\n` chars to get row offset

**Step 3 — Structured ToolEntry**
- Define `ToolEntry { name: String, params: serde_json::Value, result: Option<String>, is_error: bool, expanded: bool }` in `tui/mod.rs`
- **Important:** `TurnEvent::ToolComplete` currently has no `is_error` field. The builder must add `is_error: bool` to `TurnEvent::ToolComplete` in `types.rs` and update `turn.rs` to carry it (from `exec_result.is_error`)
- Replace `tool_events: Vec<String>` with `tool_entries: Vec<ToolEntry>`, `selected_tool: Option<usize>`
- Update `handle_ui_event`: `ToolStart` → push new `ToolEntry`; `ToolComplete` → find last entry by name and set result/is_error
- Update `events.rs`: `[`/`]` cycle `selected_tool`; `e` toggles `expanded` on selected
- Update `ui.rs`: `render_tool_panel` rewrites to collapsed/expanded view
- Update `headless()` and existing tests

**Step 4 — ChatEntry/ChatBlock**
- Define `ChatBlock { Text(String), Code { lang: String, body: String } }` and `ChatEntry { User(String), AssistantStreaming(String), AssistantDone(Vec<ChatBlock>) }` in `tui/mod.rs`
- Implement `pub fn parse_chat_blocks(text: &str) -> Vec<ChatBlock>` (must be public for tests)
- Replace `messages: Vec<String>` with `chat_history: Vec<ChatEntry>`
- `handle_submit`: push `ChatEntry::User(trimmed)` before spawning
- `handle_ui_event` `TextChunk`: if last entry is `AssistantStreaming`, append text; else push new `AssistantStreaming`
- `handle_ui_event` `TurnEnd`: convert last `AssistantStreaming` → `AssistantDone(parse_chat_blocks(&text))`
- **Edge case (TurnEnd without streaming):** if last entry is NOT `AssistantStreaming` (e.g. TurnEnd after only tool calls), do NOT push a spurious `AssistantDone` — just leave `chat_history` as-is
- `render_conversation`: iterate `chat_history` → build `Vec<Line>`, code blocks get `bg(Rgb(30,30,30))`
- Public helper `pub fn chat_blocks_to_lines(blocks: &[ChatBlock]) -> Vec<Line>` for testability

**Step 5 — Auto-Scroll**
- Add `scroll_pinned: bool` (default `true`) to `TuiApp`
- In `handle_ui_event`, when `scroll_pinned`, set `scroll_offset = usize::MAX`
- In `events.rs`: `j`/`PageDown` and `k`/`PageUp` set `scroll_pinned = false`; `G` sets `scroll_pinned = true` and `scroll_offset = usize::MAX`

### Dependencies Between Steps

- Step 2 has no dependencies on Step 1
- Step 3 requires adding `is_error` to `TurnEvent::ToolComplete` (touches `types.rs` and `turn.rs`)
- Step 4 replaces `messages: Vec<String>` — must remove references in handle_submit and handle_ui_event 
- Step 5 adds a field and updates event/scroll handlers — orthogonal to 3 and 4

### Constraints

1. **No new Cargo.toml deps** — no syntect, no syntax highlighting library
2. **Clippy strict mode** — `unwrap_used`, `expect_used`, `panic` denied outside `#[cfg(test)]`
3. **`headless()` updated each step** — it mirrors all fields, any new field not in headless = compile error
4. **`turn_event_variants_are_clonable` test** — currently hardcodes `len() == 5`, must be updated to 6 when `Usage` is added
5. **`parse_chat_blocks` must be `pub`** — needed for unit tests in mod tests block
6. **scroll `u16` cast** — `usize::MAX as u16 = 65535`; Ratatui will clamp to available area, which is correct behavior
7. **`ToolComplete` `is_error` field** — currently missing from `TurnEvent`. Step 3 must add it to avoid string-based detection
8. **AssistantStreaming → AssistantDone on TurnEnd** — only convert if last entry IS `AssistantStreaming`; tool-only turns should not create empty `AssistantDone`

### Test Harness Notes

- All tests use `TuiApp::headless()` + direct method calls — no terminal, no tokio runtime needed for most tests
- `render_*` functions are private — tests for rendering correctness require public free functions
- Existing tests in `mod.rs` and `events.rs` must continue to pass at each step

## File Map for Builder

| File | What changes |
|------|-------------|
| `ap/src/types.rs` | Add `TurnEvent::Usage`; add `is_error: bool` to `TurnEvent::ToolComplete`; update tests |
| `ap/src/turn.rs` | Capture token fields in `StreamEvent::TurnEnd`; pass `is_error` to `TurnEvent::ToolComplete` |
| `ap/src/tui/mod.rs` | Each step adds/replaces fields + handle_ui_event arms + headless() + new tests |
| `ap/src/tui/events.rs` | Step 2: Enter/Ctrl+Enter; Step 3: [/]/e; Step 5: j/k unpin, G repin |
| `ap/src/tui/ui.rs` | Step 1: status bar; Step 2: dynamic height; Step 3: render_tool_panel; Step 4: render_conversation |
