---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: scroll_pinned Auto-Scroll Anchor

## Description
Add `scroll_pinned: bool` to `TuiApp`. When pinned and new content arrives, automatically scroll to `usize::MAX` (bottom). Unpin when `j`/`k`/`PageUp`/`PageDown` are pressed. Re-pin when `G` is pressed.

## Background
Currently scroll is entirely manual. The spec requires that new content auto-scrolls the view to the bottom unless the user has manually scrolled up. `j`/`k` should unpin; `G` should re-pin and snap to bottom.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/richer-tui/design.md

**Additional References:**
- .agents/scratchpad/implementation/richer-tui/context.md (codebase patterns)
- .agents/scratchpad/implementation/richer-tui/plan.md (overall strategy)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Add `scroll_pinned: bool` to `TuiApp` struct in `ap/src/tui/mod.rs`
2. Initialise `scroll_pinned: true` in both `new()` and `headless()` constructors
3. In `handle_ui_event`: on content events (`TextChunk`, `ToolStart`, `ToolComplete`, `TurnEnd`), if `scroll_pinned` is true, set `scroll_offset = usize::MAX`
4. In `handle_key_event` Normal mode in `ap/src/tui/events.rs`:
   - `j` / `PageDown` → set `scroll_pinned = false`, then increment `scroll_offset` as before
   - `k` / `PageUp` → set `scroll_pinned = false`, then decrement `scroll_offset` as before
   - `G` → set `scroll_pinned = true`, set `scroll_offset = usize::MAX`
5. Update existing scroll-related tests to account for the new `scroll_pinned` field
6. Add new pinning-specific tests
7. All code must compile with zero warnings and pass `cargo test`

## Dependencies
- Depends on: task-04 (Step 4 must be complete and compiling)

## Implementation Approach
1. Write failing tests first:
   - `scroll_pinned_sets_max_offset_on_text_chunk`: pinned + `TextChunk` → `scroll_offset == usize::MAX`
   - `scroll_j_unpins`: `j` key → `scroll_pinned == false`
   - `scroll_k_unpins`: `k` key → `scroll_pinned == false`
   - `scroll_G_repins`: `G` key → `scroll_pinned == true`, `scroll_offset == usize::MAX`
2. Add `scroll_pinned` field and initialisation
3. Update `handle_ui_event` content event handlers
4. Update `handle_key_event` for `j`, `k`, `G`
5. Fix existing scroll tests
6. Run `cargo test` — all tests green

## Acceptance Criteria

1. **Auto-scroll when pinned**
   - Given `scroll_pinned == true` and `scroll_offset == 0`
   - When `TurnEvent::TextChunk` is handled
   - Then `scroll_offset == usize::MAX`

2. **j unpins**
   - Given `scroll_pinned == true`
   - When `j` is pressed in Normal mode
   - Then `scroll_pinned == false`

3. **k unpins**
   - Given `scroll_pinned == true`
   - When `k` is pressed in Normal mode
   - Then `scroll_pinned == false`

4. **G re-pins and snaps**
   - Given `scroll_pinned == false` and `scroll_offset == 10`
   - When `G` is pressed in Normal mode
   - Then `scroll_pinned == true` and `scroll_offset == usize::MAX`

5. **ToolStart and ToolComplete also trigger auto-scroll**
   - Given `scroll_pinned == true`
   - When `TurnEvent::ToolStart` or `TurnEvent::ToolComplete` is handled
   - Then `scroll_offset == usize::MAX`

6. **All Tests Pass**
   - Given the complete implementation
   - When running `cargo test` in `ap/`
   - Then all tests pass with zero failures

## Metadata
- **Complexity**: Low
- **Labels**: tui, scroll, ux
- **Required Skills**: Rust, ratatui
