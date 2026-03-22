---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Token Usage — TurnEvent::Usage + Status Bar

## Description
Add `TurnEvent::Usage { input_tokens: u32, output_tokens: u32 }` to the event system, wire it from the existing `StreamEvent::TurnEnd` (which already carries token data but discards it), accumulate totals in `TuiApp`, and render `Tokens: ↑Xk ↓Yk │ Cost: $N.NNNN` in the status bar using hard-coded Claude 3.5 Sonnet pricing constants.

## Background
`StreamEvent::TurnEnd` in `turn.rs` already carries `input_tokens` and `output_tokens` fields but they are discarded. The `TurnEvent` enum in `types.rs` has no `Usage` variant yet. The status bar in `tui/ui.rs` currently shows no token information.

The existing test `turn_event_variants_are_clonable` in `types.rs` hardcodes `assert_eq!(cloned.len(), 5)` — this MUST be updated to `6` when `Usage` is added.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/richer-tui/design.md

**Additional References:**
- .agents/scratchpad/implementation/richer-tui/context.md (codebase patterns)
- .agents/scratchpad/implementation/richer-tui/plan.md (overall strategy)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Add `TurnEvent::Usage { input_tokens: u32, output_tokens: u32 }` to `ap/src/types.rs`
2. Update `turn_event_variants_are_clonable` test — add `Usage` to the slice, change `len` check from 5 to 6
3. In `ap/src/turn.rs`, emit `TurnEvent::Usage { input_tokens, output_tokens }` when handling `StreamEvent::TurnEnd`
4. Add `total_input_tokens: u32` and `total_output_tokens: u32` to `TuiApp` struct in `ap/src/tui/mod.rs`
5. Initialise both fields to `0` in `new()` and `headless()` constructors
6. Handle `TurnEvent::Usage` in `handle_ui_event` by accumulating into the totals
7. In `ap/src/tui/ui.rs`, add pricing constants `COST_PER_M_INPUT: f64 = 3.00` and `COST_PER_M_OUTPUT: f64 = 15.00`
8. Render status bar as `Tokens: ↑{input_k}k ↓{output_k}k │ Cost: ${cost:.4}` where `input_k` and `output_k` are in thousands
9. All code must compile with zero warnings and pass `cargo test`

## Dependencies
- None (this is Step 1)

## Implementation Approach
1. Write failing unit tests first:
   - `handle_ui_event_usage_accumulates`: send two `Usage` events, verify accumulated sums
   - `status_bar_format`: verify cost string formatting with f64 arithmetic
2. Add `TurnEvent::Usage` variant and update the clone test
3. Wire the emission in `turn.rs`
4. Add fields to `TuiApp` and handle in `handle_ui_event`
5. Update status bar rendering in `ui.rs`
6. Run `cargo test` — all tests green

## Acceptance Criteria

1. **Usage variant exists**
   - Given the `TurnEvent` enum in `types.rs`
   - When searching for the `Usage` variant
   - Then `TurnEvent::Usage { input_tokens: u32, output_tokens: u32 }` exists and is Clone + Debug

2. **Token accumulation**
   - Given a `TuiApp` with `total_input_tokens=0` and `total_output_tokens=0`
   - When two `TurnEvent::Usage { input_tokens: 100, output_tokens: 200 }` events are handled
   - Then `total_input_tokens == 200` and `total_output_tokens == 400`

3. **Status bar format**
   - Given `total_input_tokens=1000` and `total_output_tokens=2000`
   - When the status bar is rendered
   - Then it displays `Tokens: ↑1k ↓2k │ Cost: $0.0330`

4. **Clone test updated**
   - Given the `turn_event_variants_are_clonable` test
   - When it runs
   - Then it asserts `len == 6` (not 5) and includes a `Usage` variant

5. **All Tests Pass**
   - Given the complete implementation
   - When running `cargo test` in `ap/`
   - Then all tests pass with zero failures

## Metadata
- **Complexity**: Low
- **Labels**: types, tui, status-bar, tokens
- **Required Skills**: Rust, ratatui
