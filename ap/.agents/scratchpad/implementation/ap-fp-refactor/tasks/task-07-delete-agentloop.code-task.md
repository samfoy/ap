---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: Delete `src/app.rs` — Remove `AgentLoop` and `UiEvent`

## Description
Delete `src/app.rs` and remove all references to `AgentLoop` and `UiEvent` throughout the codebase. This is the final cleanup step — by this point `AgentLoop` is no longer used by `main.rs` or `tui/mod.rs`, so deleting it should be straightforward.

## Background
After Tasks 05 and 06, `AgentLoop` is fully orphaned. `UiEvent` has been replaced by `TurnEvent`. This step confirms there are zero remaining references and completes the FP refactor structural cleanup.

## Reference Documentation
**Required:**
- Design/Plan: ap/.agents/scratchpad/implementation/ap-fp-refactor/plan.md

**Additional References:**
- ap/.agents/scratchpad/implementation/ap-fp-refactor/context.md

**Note:** You MUST read the plan document before beginning implementation.

## Technical Requirements
1. Verify no remaining usage before deleting:
   ```bash
   grep -r "AgentLoop\|UiEvent\|use ap::app\|mod app" ap/src ap/tests
   ```
   Must return zero matches.
2. Delete `src/app.rs`
3. Remove `pub mod app;` from `src/lib.rs`
4. Remove any residual imports of `AgentLoop` or `UiEvent` anywhere they still appear
5. Run `cargo build --release` — zero warnings
6. Run `cargo clippy -- -D warnings` — zero warnings
7. Run `cargo test` — all tests pass

## Dependencies
- Task 05: main.rs no longer uses AgentLoop for headless
- Task 06: TUI no longer uses AgentLoop

## Implementation Approach
1. Grep for all references — if any remain, fix them first
2. Delete app.rs
3. Remove pub mod app from lib.rs
4. Build + test

## Acceptance Criteria

1. **AgentLoop is gone**
   - Given the completed implementation
   - When running `grep -r "AgentLoop" ap/src ap/tests`
   - Then zero matches are returned

2. **UiEvent is gone**
   - Given the completed implementation
   - When running `grep -r "UiEvent" ap/src ap/tests`
   - Then zero matches are returned

3. **app.rs is deleted**
   - Given the completed implementation
   - When running `ls ap/src/app.rs`
   - Then the file does not exist (command returns non-zero)

4. **Release build is clean**
   - Given the implementation is complete
   - When running `cargo build --release`
   - Then it compiles with zero warnings

5. **Clippy is clean**
   - Given the implementation is complete
   - When running `cargo clippy -- -D warnings`
   - Then zero warnings or errors

6. **All tests pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all tests pass with zero failures

## Metadata
- **Complexity**: Low
- **Labels**: cleanup, fp-refactor
- **Required Skills**: Rust
