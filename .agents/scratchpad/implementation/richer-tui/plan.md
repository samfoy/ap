# Implementation Plan — Richer TUI

Source spec: `/Users/sam.painter/Projects/ap/PROMPT.md`

## Key Constraints (from research)

1. `TurnEvent::ToolComplete` currently has no `is_error` field — must be added in Step 3
2. `turn_event_variants_are_clonable` test in `types.rs` hardcodes `assert_eq!(cloned.len(), 5)` — must be updated to 6 when `TurnEvent::Usage` is added in Step 1
3. `parse_chat_blocks` must be `pub` (needed for unit test in Step 4)
4. All work is in `ap/src/` — the crate under `/Users/sam.painter/Projects/ap-worktrees/richer-tui/ap/`

## Test Strategy

### Unit Tests (per step)

**Step 1 — Token Usage**
- `handle_ui_event_usage_accumulates` — two `Usage` events, verify sums
- `status_bar_format` — verify cost string formatting (f64 arithmetic)
- Update `turn_event_variants_are_clonable` to `len==6` and add `Usage` to the slice

**Step 2 — Multi-line Input**
- `insert_mode_enter_adds_newline` — `Enter` pushes `'\n'`, buffer not cleared
- `insert_mode_ctrl_enter_submits` — `Ctrl+Enter` drains buffer → `Action::Submit`
- `input_box_height_min` — empty buffer → height 4 (2 lines clamped, +2 border)
- `input_box_height_max` — 10 newlines → height 8 (6 lines clamped, +2 border)
- Update existing `insert_mode_enter_submits_buffer` test → rename/replace

**Step 3 — Structured Tool Entries**
- `tool_entry_start_creates_running_entry` — ToolStart pushes entry with result=None
- `tool_entry_complete_fills_result` — ToolComplete fills matching entry
- `tool_entry_expand_toggle` — `e` toggles `expanded`; unselected `e` is no-op
- `tool_selection_bracket_keys` — `]` moves forward, `[` moves back, wraps at bounds
- `tool_entry_is_error_from_turn_event` — ToolComplete with is_error=true sets entry.is_error
- Remove/update old `tool_events`-based tests

**Step 4 — Structured Chat History + Syntax Highlighting**
- `parse_chat_blocks_no_fence` — plain text → `[Text(s)]`
- `parse_chat_blocks_single_fence` — text + fence → `[Text, Code]`
- `parse_chat_blocks_with_lang` — ` ```rust ` tag captured in Code.lang
- `parse_chat_blocks_unclosed_fence` — unclosed ``` treated as code block
- `parse_chat_blocks_empty` — empty string → empty vec
- `streaming_lifecycle_ends_as_done` — TextChunk × N then TurnEnd → AssistantDone
- `streaming_lifecycle_chunks_appended` — multiple TextChunks land in AssistantStreaming
- Remove/update old `messages`-based tests

**Step 5 — Auto-Scroll Anchor**
- `scroll_pinned_sets_max_offset_on_text_chunk` — pinned + TextChunk → offset=usize::MAX
- `scroll_j_unpins` — `j` sets scroll_pinned=false
- `scroll_k_unpins` — `k` sets scroll_pinned=false
- `scroll_G_repins` — `G` sets scroll_pinned=true, offset=usize::MAX
- Update existing scroll tests

### Integration Points
- `types.rs` → `turn.rs` (emit Usage from StreamEvent::TurnEnd)
- `types.rs` → `tui/mod.rs` (handle new TurnEvent variants)
- `tui/mod.rs` ↔ `tui/events.rs` (new key bindings reference new struct fields)
- `tui/mod.rs` ↔ `tui/ui.rs` (render new data structures)

### E2E Scenario (Validator will execute manually)

**Harness:** Run `cargo run` in the `ap/` directory against a real terminal (or use `cargo test` for the automated parts).

**Happy path:**
1. Launch `ap` (or run `cargo test` to verify all unit tests pass)
2. Enter Insert mode (`i`), type a multi-line message using `Enter` to add newlines
3. Verify input box grows (min 4, max 8 rows)
4. Submit with `Ctrl+Enter`
5. Verify status bar shows `Tokens: ↑Xk ↓Yk │ Cost: $N.NNNN` after turn completes
6. Verify assistant response with a code block renders with dark background lines
7. Use `]`/`[` to navigate tool entries; press `e` to expand — verify params/result shown
8. Press `j` to scroll up (unpin), then `G` to snap back to bottom (re-pin)

**Adversarial paths:**
- `]` with empty tool_entries → no panic, selected_tool stays None
- `e` with no selection → no panic
- Very long code block → truncated to panel width, no overflow
- 10+ newlines in input → height capped at 8 rows

---

## Implementation Steps

Each step must leave `cargo build` and `cargo test` clean.

### Step 1 — Token Usage (types + turn + status bar)

**Files:** `ap/src/types.rs`, `ap/src/turn.rs`, `ap/src/tui/mod.rs`, `ap/src/tui/ui.rs`

1. Add `TurnEvent::Usage { input_tokens: u32, output_tokens: u32 }` to `types.rs`
2. Update `turn_event_variants_are_clonable` test — add `Usage` to slice, change len check to 6
3. Emit `TurnEvent::Usage` in `turn.rs` on `StreamEvent::TurnEnd`
4. Add `total_input_tokens: u32` / `total_output_tokens: u32` to `TuiApp` struct
5. Initialise both to 0 in `new()` and `headless()`
6. Handle `TurnEvent::Usage` in `handle_ui_event`
7. Update status bar in `ui.rs` with `COST_PER_M_INPUT=3.00` / `COST_PER_M_OUTPUT=15.00` constants
8. Add unit test `handle_ui_event_usage_accumulates`
9. `cargo test` green

**Demo after step:** Status bar shows live token/cost totals (verified by unit test; visual confirmation next run).

### Step 2 — Multi-line Input

**Files:** `ap/src/tui/events.rs`, `ap/src/tui/ui.rs`

1. In `handle_key_event` Insert mode: `Ctrl+Enter` → `Action::Submit(drain)`, plain `Enter` → `push('\n')`
2. Rename/replace `insert_mode_enter_submits_buffer` test with two new tests
3. Add `input_box_height(app: &TuiApp) -> u16` to `ui.rs`; use in layout constraint
4. Fix cursor positioning for multi-line (x = chars after last `\n`, y = border + line index)
5. Update help overlay keybinding table
6. Add tests `insert_mode_enter_adds_newline`, `insert_mode_ctrl_enter_submits`, height tests
7. `cargo test` green

**Demo after step:** `Enter` in insert mode adds newlines; `Ctrl+Enter` submits; box grows.

### Step 3 — Structured Tool Entries

**Files:** `ap/src/types.rs`, `ap/src/tui/mod.rs`, `ap/src/tui/events.rs`, `ap/src/tui/ui.rs`

1. Add `is_error: bool` to `TurnEvent::ToolComplete` in `types.rs`; update all match arms
2. Define `ToolEntry` struct in `tui/mod.rs`
3. Replace `tool_events: Vec<String>` with `tool_entries: Vec<ToolEntry>` + `selected_tool: Option<usize>` in `TuiApp`
4. Update `headless()` constructor; fix all `tool_events` references in existing tests
5. Update `handle_ui_event` for `ToolStart` / `ToolComplete`
6. Add `[`, `]`, `e` key handlers in Normal mode events
7. Rewrite `render_tool_panel` in `ui.rs`
8. Update help overlay text
9. Add new tests
10. `cargo test` green

**Demo after step:** Tool panel shows structured entries with expand/collapse.

### Step 4 — Structured Chat History + Syntax Highlighting

**Files:** `ap/src/tui/mod.rs`, `ap/src/tui/ui.rs`

1. Define `ChatBlock` and `ChatEntry` enums in `tui/mod.rs`
2. Replace `messages: Vec<String>` with `chat_history: Vec<ChatEntry>` in `TuiApp`
3. Update `headless()` constructor; fix all `messages` references in existing tests
4. Implement `pub fn parse_chat_blocks(text: &str) -> Vec<ChatBlock>`
5. Update `handle_ui_event` streaming lifecycle
6. Update `handle_submit` to push `ChatEntry::User`
7. Rewrite `render_conversation` in `ui.rs` to convert `chat_history` to `Vec<Line>` with code styling
8. Add tests for `parse_chat_blocks` and streaming lifecycle
9. `cargo test` green

**Demo after step:** Code blocks in assistant messages render with dark background.

### Step 5 — Auto-Scroll Anchor

**Files:** `ap/src/tui/mod.rs`, `ap/src/tui/events.rs`

1. Add `scroll_pinned: bool` to `TuiApp`; initialise `true` in `new()` and `headless()`
2. In `handle_ui_event`: on content events (TextChunk, ToolStart, ToolComplete, TurnEnd), if pinned set `scroll_offset = usize::MAX`
3. In `handle_key_event` Normal mode: `j`/`PageDown` → unpin + scroll; `k`/`PageUp` → unpin + scroll; `G` → re-pin + offset=MAX
4. Update scroll-related tests
5. Add new pinning tests
6. `cargo test` green

**Demo after step:** New content auto-scrolls; `j`/`k` freeze scroll; `G` jumps to bottom.

---

## Success Criteria

All 13 ACs from PROMPT.md must be met:
- AC-1: `cargo build` zero warnings
- AC-2: all `cargo test` pass
- AC-3: Usage event emitted with correct tokens
- AC-4: Status bar format correct
- AC-5: Enter=newline, Ctrl+Enter=submit
- AC-6: Input box height dynamic 4-8
- AC-7: ToolEntry struct used everywhere
- AC-8: `[`/`]` cycle, `e` toggles
- AC-9: AssistantDone on TurnEnd
- AC-10: parse_chat_blocks ≥4 unit tests
- AC-11: Code block lines with bg(Rgb(30,30,30))
- AC-12: scroll_pinned behavior
- AC-13: clippy deny lints satisfied outside test blocks
