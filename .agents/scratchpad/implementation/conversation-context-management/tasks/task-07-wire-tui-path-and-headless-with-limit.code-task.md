---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Wire TUI Path + headless_with_limit Constructor

## Description
Complete the end-to-end pipeline by wiring `maybe_compress_context` into the TUI's `handle_submit` spawned task, adding `context_limit: Option<u32>` to `TuiApp::new`, updating `run_tui` to pass the config value, and adding `TuiApp::headless_with_limit` for tests.

## Background
The TUI path spawns an async task in `handle_submit` that calls `turn()`. After this step, it will: build `conv_with_msg` → clone as `fallback` → conditionally call `maybe_compress_context` → send `ContextSummarized` event via `tx` → call `turn()`. All values captured in the closure must be `Copy` scalars (u32, f32, usize) — NOT references.

`TuiApp::headless()` is unchanged (24 call sites in tests). A new `TuiApp::headless_with_limit(context_limit: Option<u32>) -> Self` constructor is added for tests that need to exercise the compression UI path. `headless()` can internally delegate to `headless_with_limit(None)` or remain standalone — either is acceptable.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/conversation-context-management/design.md`

**Additional References:**
- `.agents/scratchpad/implementation/conversation-context-management/context.md` (codebase patterns, TuiApp patterns)
- `.agents/scratchpad/implementation/conversation-context-management/plan.md` (overall strategy)

**Note:** You MUST read the design document (§4.4 TUI path) before beginning implementation. Read `ap/src/tui/mod.rs` and `ap/src/main.rs` `run_tui` in full before making changes.

## Technical Requirements
1. **`ap/src/tui/mod.rs`**:
   - Add `context_limit: Option<u32>` parameter to `TuiApp::new`; store as `self.context_limit`
   - Add `pub fn headless_with_limit(context_limit: Option<u32>) -> Self` — same as `headless()` but sets `self.context_limit = context_limit`
   - `headless()` remains UNCHANGED (all 24 existing call sites must compile without modification)
   - In `handle_submit` spawned task:
     - `let fallback = conv_with_msg.clone();`
     - Guard with `if let Some(limit) = self.context_limit`
     - Capture `limit: u32`, `keep_recent: usize`, `threshold: f32` as `Copy` scalars into the closure
     - On `Ok((c, Some(evt)))` → `tx.send(evt).await.ok()`, use `c`
     - On `Ok((c, None))` → use `c`
     - On `Err(e)` → `tx.send(TurnEvent::Error(e.to_string())).await.ok(); return`
2. **`ap/src/main.rs`**: Update `run_tui` to pass `config.context.limit` to `TuiApp::new`
3. **All 24 existing `headless()` call sites compile without modification**

## Dependencies
- Task 04 (`last_input_tokens`, `context_limit` fields on TuiApp must exist)
- Task 05 (`maybe_compress_context` must exist)
- Task 06 (headless path wired; same ownership pattern)

## Implementation Approach
1. **TDD: Write the 1 failing test first** (`tuiapp_new_stores_context_limit`)
2. Add `context_limit` parameter to `TuiApp::new`
3. Add `headless_with_limit` constructor
4. Wire the spawned task in `handle_submit`
5. Update `run_tui` in `main.rs`
6. `cargo test` — all 26 tests pass (1 new + 25 cumulative); all existing tests pass
7. `cargo clippy -- -D warnings` — zero warnings
8. `cargo build --release` — zero errors, zero warnings

## Acceptance Criteria

1. **tuiapp_new_stores_context_limit**
   - Given `TuiApp::headless_with_limit(Some(50_000))`
   - When inspecting `app.context_limit`
   - Then `app.context_limit == Some(50_000)`

2. **Existing headless() Call Sites Unaffected**
   - Given 24 existing `TuiApp::headless()` calls in tests
   - When running `cargo test`
   - Then all compile and pass without modification

3. **TUI Wired End-to-End**
   - Given a TUI session with `--context-limit` set
   - When a turn is submitted and compression fires
   - Then `ContextSummarized` event is sent via `tx`, `handle_ui_event` processes it, and a notice appears in chat history

4. **run_tui Passes Limit**
   - Given `config.context.limit == Some(50000)`
   - When `run_tui` constructs `TuiApp::new`
   - Then `app.context_limit == Some(50000)`

5. **All 26 Tests Pass**
   - Given the full implementation is complete
   - When running `cargo test`
   - Then all 26 new tests pass and all pre-existing tests pass

6. **Clippy and Release Build Clean**
   - Given the implementation is complete
   - When running `cargo clippy -- -D warnings` and `cargo build --release`
   - Then zero warnings and zero errors

## Metadata
- **Complexity**: Medium
- **Labels**: context-management, tui, wiring, end-to-end, tdd
- **Required Skills**: Rust, async/await, ratatui, ownership patterns
