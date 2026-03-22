---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Multi-line Input — Enter=newline, Ctrl+Enter=submit

## Description
Remap keyboard handling in Insert mode so `Enter` inserts a newline character into the input buffer instead of submitting, while `Ctrl+Enter` triggers submission. Make the input box height dynamic (min 4, max 8 rows) and fix multi-line cursor positioning.

## Background
Currently in `ap/src/tui/events.rs`, the `Enter` key in Insert mode calls `Action::Submit`. The prompt spec requires `Enter` to add `\n` to the buffer and `Ctrl+Enter` to submit. The input box height in `ui.rs` is currently fixed — it needs to grow with content up to 8 rows.

The existing test `insert_mode_enter_submits_buffer` must be renamed/replaced with two new tests.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/richer-tui/design.md

**Additional References:**
- .agents/scratchpad/implementation/richer-tui/context.md (codebase patterns)
- .agents/scratchpad/implementation/richer-tui/plan.md (overall strategy)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. In `handle_key_event` Insert mode in `ap/src/tui/events.rs`: `Ctrl+Enter` → `Action::Submit(drain)`, plain `Enter` → push `'\n'` into buffer
2. Remove or rename the old `insert_mode_enter_submits_buffer` test
3. Add `input_box_height(app: &TuiApp) -> u16` helper in `ap/src/tui/ui.rs`
   - Count newlines + 1 for total lines
   - Clamp to range 2..=6 (content lines), then add 2 (borders) → result 4..=8
4. Use `input_box_height` in the layout constraint for the input area
5. Fix cursor positioning for multi-line input:
   - `x` = number of chars after the last `\n`
   - `y` = border offset + line index (count of `\n` before cursor)
6. Update the help overlay keybinding table to reflect new mappings
7. All code must compile with zero warnings and pass `cargo test`

## Dependencies
- Depends on: task-01 (Step 1 must be complete and compiling)

## Implementation Approach
1. Write failing tests first:
   - `insert_mode_enter_adds_newline`: `Enter` pushes `'\n'`, buffer is NOT cleared
   - `insert_mode_ctrl_enter_submits`: `Ctrl+Enter` drains buffer → `Action::Submit`
   - `input_box_height_min`: empty buffer → height 4
   - `input_box_height_max`: 10 newlines in buffer → height 8
2. Update key handling in `events.rs`
3. Add `input_box_height` to `ui.rs` and wire into layout
4. Fix cursor positioning
5. Run `cargo test` — all tests green

## Acceptance Criteria

1. **Enter inserts newline**
   - Given Insert mode is active with buffer `"hello"`
   - When `Enter` is pressed
   - Then buffer becomes `"hello\n"` and the app does NOT submit

2. **Ctrl+Enter submits**
   - Given Insert mode is active with buffer `"hello\nworld"`
   - When `Ctrl+Enter` is pressed
   - Then `Action::Submit` is returned with value `"hello\nworld"` and buffer is cleared

3. **Input box minimum height**
   - Given the input buffer is empty
   - When `input_box_height` is called
   - Then it returns `4`

4. **Input box maximum height**
   - Given the input buffer contains 10 or more newlines
   - When `input_box_height` is called
   - Then it returns `8`

5. **All Tests Pass**
   - Given the complete implementation
   - When running `cargo test` in `ap/`
   - Then all tests pass with zero failures

## Metadata
- **Complexity**: Low
- **Labels**: tui, input, keyboard, layout
- **Required Skills**: Rust, ratatui
