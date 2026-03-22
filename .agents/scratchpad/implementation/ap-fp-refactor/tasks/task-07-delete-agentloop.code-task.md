---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Delete AgentLoop

## Description
Remove the legacy `AgentLoop` struct, `UiEvent` enum, and all associated tests.
Since Step 06 decoupled `TuiApp` and `main.rs` from `AgentLoop`, the old code is now
dead — no callers remain outside of its own test file and the two legacy integration test files.

## Background
- `src/app.rs` contains `AgentLoop` (mutable orchestration) and `UiEvent` (old event enum)
- `tests/agent_loop.rs` and `tests/hook_cancel.rs` test `AgentLoop` exclusively
- After Step 06, `main.rs`, `tui/mod.rs`, and all integration tests use `turn()` + `TurnEvent`
- `pub mod app;` in `lib.rs` is the only pub exposure remaining

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-fp-refactor/progress.md

**Additional References:**
- src/types.rs (TurnEvent — the new event type that replaced UiEvent)
- src/turn.rs (pure turn() pipeline — the new orchestrator)

**Note:** Verify no callers remain before deleting.

## Technical Requirements
1. Verify zero remaining references to `AgentLoop`, `UiEvent`, or `app::` outside `src/app.rs` and the two legacy test files
2. Delete `src/app.rs`
3. Delete `tests/agent_loop.rs`
4. Delete `tests/hook_cancel.rs`
5. Remove `pub mod app;` from `src/lib.rs`
6. `cargo build --release` and `cargo test` must pass after deletion

## Dependencies
- Steps 01-06 must be complete (they are)

## Implementation Approach
1. `grep -rn "AgentLoop\|UiEvent\|app::"` to confirm no references outside the 3 files
2. Delete the 3 files
3. Remove the `pub mod app;` line from `lib.rs`
4. Run `cargo build --release` — fix any residual errors
5. Run `cargo test` — confirm all remaining tests pass
6. Commit with message `refactor: delete AgentLoop and legacy UiEvent`

## Acceptance Criteria

1. **No dead code remains**
   - Given the refactor is complete
   - When running `grep -rn "AgentLoop\|UiEvent" ap/src/ ap/tests/`
   - Then the output is empty

2. **Build succeeds**
   - Given `src/app.rs`, `tests/agent_loop.rs`, and `tests/hook_cancel.rs` are deleted
   - When running `cargo build --release`
   - Then exit code is 0, zero warnings

3. **All remaining tests pass**
   - Given the legacy test files are deleted
   - When running `cargo test`
   - Then all remaining tests pass (count will be less than 107 — the agent_loop and hook_cancel tests are intentionally removed)

4. **lib.rs clean**
   - Given `src/app.rs` is deleted
   - When reading `src/lib.rs`
   - Then there is no `pub mod app;` line

5. **turn.rs and types.rs untouched**
   - Given this is a deletion-only task
   - When diffing src/turn.rs and src/types.rs
   - Then no changes are present

## Metadata
- **Complexity**: Low
- **Labels**: cleanup, refactor, deletion
- **Required Skills**: Rust
