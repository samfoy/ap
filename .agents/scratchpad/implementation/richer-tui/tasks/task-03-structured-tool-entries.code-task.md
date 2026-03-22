---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Structured Tool Entries — Replace Vec<String>

## Description
Replace the flat `tool_events: Vec<String>` in `TuiApp` with a structured `Vec<ToolEntry>` where each entry tracks name, params, result, error state, and expansion state. Add keyboard navigation (`[`/`]`) and toggle-expansion (`e`) in Normal mode. Also add `is_error: bool` to `TurnEvent::ToolComplete`.

## Background
Currently `TuiApp.tool_events` is a `Vec<String>` holding raw text lines. The spec requires a `ToolEntry` struct with rich fields. `TurnEvent::ToolComplete` in `types.rs` currently has no `is_error` field — this must be added. All existing match arms on `ToolComplete` must be updated.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/richer-tui/design.md

**Additional References:**
- .agents/scratchpad/implementation/richer-tui/context.md (codebase patterns)
- .agents/scratchpad/implementation/richer-tui/plan.md (overall strategy)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Add `is_error: bool` to `TurnEvent::ToolComplete` in `ap/src/types.rs`; update all match arms throughout the codebase
2. Define `ToolEntry` struct in `ap/src/tui/mod.rs`:
   ```rust
   pub struct ToolEntry {
       pub name: String,
       pub params: String,
       pub result: Option<String>,
       pub is_error: bool,
       pub expanded: bool,
   }
   ```
3. Replace `tool_events: Vec<String>` with `tool_entries: Vec<ToolEntry>` and add `selected_tool: Option<usize>` in `TuiApp`
4. Update `headless()` constructor to initialise new fields
5. Update `handle_ui_event` for `ToolStart` (push new running entry) and `ToolComplete` (fill result/is_error on matching entry)
6. Add `[`, `]`, `e` key handlers in Normal mode in `ap/src/tui/events.rs`
7. Rewrite `render_tool_panel` in `ap/src/tui/ui.rs` to render collapsed/expanded entries with selection highlight
8. Update help overlay text
9. Fix all `tool_events` references in existing tests
10. All code must compile with zero warnings and pass `cargo test`

## Dependencies
- Depends on: task-02 (Step 2 must be complete and compiling)

## Implementation Approach
1. Write failing tests first:
   - `tool_entry_start_creates_running_entry`: `ToolStart` pushes entry with `result=None`
   - `tool_entry_complete_fills_result`: `ToolComplete` fills matching entry
   - `tool_entry_expand_toggle`: `e` toggles `expanded`; unselected `e` is no-op
   - `tool_selection_bracket_keys`: `]` moves forward, `[` moves back, wraps at bounds
   - `tool_entry_is_error_from_turn_event`: `ToolComplete` with `is_error=true` sets entry.is_error
2. Add `is_error` to `ToolComplete` and update all match arms
3. Define `ToolEntry` struct and update `TuiApp`
4. Update `handle_ui_event` handlers
5. Add key bindings
6. Rewrite render
7. Run `cargo test` — all tests green

## Acceptance Criteria

1. **ToolStart creates running entry**
   - Given an empty `tool_entries` list
   - When `TurnEvent::ToolStart { name: "read", params: "{}" }` is handled
   - Then `tool_entries` has one entry with `name="read"`, `params="{}"`, `result=None`, `is_error=false`, `expanded=false`

2. **ToolComplete fills result**
   - Given a `tool_entries` list with one entry named `"read"` and `result=None`
   - When `TurnEvent::ToolComplete { name: "read", result: "contents", is_error: false }` is handled
   - Then the entry's `result == Some("contents")` and `is_error == false`

3. **is_error propagates**
   - Given a running entry for `"bash"`
   - When `TurnEvent::ToolComplete { name: "bash", result: "error msg", is_error: true }` is handled
   - Then the entry's `is_error == true`

4. **Expand toggle**
   - Given `selected_tool = Some(0)` and entry at index 0 has `expanded=false`
   - When `e` is pressed in Normal mode
   - Then entry at index 0 has `expanded=true`

5. **Navigation keys**
   - Given `tool_entries` with 3 entries and `selected_tool = Some(1)`
   - When `]` is pressed
   - Then `selected_tool = Some(2)`, and pressing `]` again stays at `Some(2)` (or wraps)

6. **All Tests Pass**
   - Given the complete implementation
   - When running `cargo test` in `ap/`
   - Then all tests pass with zero failures

## Metadata
- **Complexity**: Medium
- **Labels**: tui, tools, struct, keyboard
- **Required Skills**: Rust, ratatui
