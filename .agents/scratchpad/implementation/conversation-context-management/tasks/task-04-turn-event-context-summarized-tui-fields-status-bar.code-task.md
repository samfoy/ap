---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: TurnEvent::ContextSummarized + TUI Fields + Status Bar

## Description
Add the `ContextSummarized` variant to `TurnEvent`, add `last_input_tokens` and `context_limit` fields to `TuiApp`, extend `handle_ui_event` to process the new variant and `Usage` updates, extend `render_status_bar` with a `ctx:` segment, and add the non-exhaustive `ContextSummarized` arm to `route_headless_events` in `main.rs` (required to keep the build compiling).

## Background
`TurnEvent` is a `#[non_exhaustive]` (or exhaustive) enum used to stream events from the `turn()` pipeline to both the TUI and headless route. Adding a new variant requires updating all match sites. The `Usage` event already exists and carries `input_tokens` — we now need to track the most recent `input_tokens` (not the cumulative total) to display current context size.

The status bar currently renders provider/model/cost info. The new ctx segment follows the same `│ key: value` pattern already used elsewhere.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/conversation-context-management/design.md`

**Additional References:**
- `.agents/scratchpad/implementation/conversation-context-management/context.md` (codebase patterns, TUI field/event patterns)
- `.agents/scratchpad/implementation/conversation-context-management/plan.md` (overall strategy)

**Note:** You MUST read the design document before beginning implementation. Read `ap/src/types.rs`, `ap/src/tui/mod.rs`, `ap/src/tui/ui.rs`, and `ap/src/main.rs` in full before making changes.

## Technical Requirements
1. **`ap/src/types.rs`**: Add `ContextSummarized { messages_before: usize, messages_after: usize, tokens_before: u32, tokens_after: u32 }` to `TurnEvent`; it must be `Clone` (all variants must be Clone)
2. **`ap/src/tui/mod.rs`**:
   - Add `pub last_input_tokens: u32` (default `0`) to `TuiApp`
   - Add `pub context_limit: Option<u32>` (default `None`) to `TuiApp`
   - `headless()` constructor initializes both fields to their defaults
   - `handle_ui_event` arm for `TurnEvent::Usage { input_tokens, .. }` → sets `self.last_input_tokens = input_tokens` (not cumulative; replaces on each usage event)
   - `handle_ui_event` arm for `TurnEvent::ContextSummarized { tokens_after, messages_before, messages_after, .. }` → push a chat notice message + set `self.last_input_tokens = tokens_after`
3. **`ap/src/tui/ui.rs`**: Extend `render_status_bar` to append `│ ctx: XX.Xk` (always) and `/YYYk (ZZ%)` (only when `context_limit` is `Some`); use `f32` arithmetic for `k` formatting; percentage is `(last_input_tokens as f32 / limit as f32 * 100.0) as u32`
4. **`ap/src/main.rs`**: Add `TurnEvent::ContextSummarized { .. } => { eprintln!("context summarized"); }` arm to the `route_headless_events` match to prevent a non-exhaustive compile error

## Dependencies
- Task 03 (main.rs already modified; headless path wired in Step 6)

## Implementation Approach
1. **TDD: Write all 6 failing tests first** (4 in `tui/mod.rs` tests, 2 in `tui/ui.rs` tests)
2. Add `ContextSummarized` to `TurnEvent` in `types.rs`
3. Add fields to `TuiApp` and update `headless()`
4. Extend `handle_ui_event`
5. Extend `render_status_bar` in `ui.rs`
6. Add the arm in `route_headless_events` in `main.rs`
7. `cargo test` — all tests (old + 6 new) pass; `cargo build` zero warnings

## Acceptance Criteria

1. **turn_event_context_summarized_clonable**
   - Given `TurnEvent::ContextSummarized { messages_before: 10, messages_after: 3, tokens_before: 5000, tokens_after: 500 }`
   - When calling `.clone()`
   - Then the clone compiles and the fields roundtrip correctly

2. **handle_ui_event_context_summarized_appends_notice**
   - Given a `TuiApp` in headless state
   - When calling `handle_ui_event` with `TurnEvent::ContextSummarized { .. }`
   - Then `chat_history` grows by exactly 1 entry

3. **handle_ui_event_usage_updates_last_input_tokens**
   - Given a `TuiApp` in headless state with `last_input_tokens == 0`
   - When calling `handle_ui_event` with `TurnEvent::Usage { input_tokens: 5000, .. }`
   - Then `app.last_input_tokens == 5000`

4. **handle_ui_event_usage_still_accumulates_totals**
   - Given a `TuiApp` in headless state
   - When calling `handle_ui_event` twice with `Usage { input_tokens: 3000 }` and `Usage { input_tokens: 4000 }`
   - Then the total usage counter (existing `total_input_tokens` or equivalent) accumulates both (3000 + 4000 = 7000) while `last_input_tokens == 4000`

5. **status_bar_ctx_display_no_limit**
   - Given `TuiApp` with `last_input_tokens = 45200` and `context_limit = None`
   - When rendering the status bar
   - Then the output contains `"ctx: 45.2k"` and does NOT contain `"%"`

6. **status_bar_ctx_display_with_limit**
   - Given `TuiApp` with `last_input_tokens = 45200` and `context_limit = Some(200000)`
   - When rendering the status bar
   - Then the output contains `"ctx: 45.2k/200k (23%)"`

7. **All Existing Tests Pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all pre-existing tests pass and `cargo build` produces zero warnings

## Metadata
- **Complexity**: Medium
- **Labels**: context-management, tui, events, status-bar, tdd
- **Required Skills**: Rust, ratatui, event handling
