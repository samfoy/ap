---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: Rewrite `src/main.rs` — Recipe-Style + Headless via `turn()`

## Description
Rewrite `src/main.rs` to use the new recipe-style pipeline pattern. Headless mode (`-p` flag) uses `turn()` directly. The TUI path still calls the old `AgentLoop` for now (this is intentional — TUI decoupling happens in Task 06). Rewrite `tests/noninteractive.rs` to test the new headless path via `turn()`.

## Background
`main.rs` should read like a clear recipe: build tools, build middleware, load/create conversation, dispatch to headless or TUI. The headless path is completely rewritten to use `turn()`. The TUI path temporarily retains the old `AgentLoop` bridge — this is a deliberate incremental step that keeps the system working while the TUI is decoupled in the next task.

## Reference Documentation
**Required:**
- Design/Plan: ap/.agents/scratchpad/implementation/ap-fp-refactor/plan.md

**Additional References:**
- ap/.agents/scratchpad/implementation/ap-fp-refactor/context.md (codebase patterns)
- ap/src/main.rs (current implementation to rewrite)
- ap/tests/noninteractive.rs (current tests to rewrite)

**Note:** You MUST read the plan document before beginning implementation. Pay attention to Step 5 for the exact recipe pattern.

## Technical Requirements
1. Rewrite `src/main.rs`:
   - Build ToolRegistry with `.with()` chain: `ReadTool`, `WriteTool`, `EditTool`, `BashTool`
   - Build Middleware via `shell_hook_bridge(&config.hooks)` from `middleware.rs`
   - Load or create `Conversation` (from session store if `--session` provided, else new)
   - Dispatch: if `-p` provided → `run_headless(conv, prompt, &provider, &tools, &middleware).await`, else → `run_tui(...)` (TUI path temporarily keeps using AgentLoop or adapts — as long as it compiles and works)
2. `run_headless` function:
   - Create mpsc channel for TurnEvent
   - Call `turn(conv.with_user_message(prompt), &provider, &tools, &middleware, &tx).await`
   - Drain events: TextChunk → print to stdout (flushed), ToolStart/ToolComplete → eprintln to stderr, TurnEnd → exit 0, Error → eprintln + exit 1
   - If session provided, autosave via `store.save_conversation(&new_conv)`
   - Match on turn result: Ok(Ok(())) no-op, Ok(Err(e)) → eprintln + exit 1, Err(e) → eprintln + exit 1
3. Rewrite `tests/noninteractive.rs`:
   - Import `ap::turn::turn` and `ap::types::{Conversation, TurnEvent, Middleware}`
   - Test helper `run_headless_test(prompt, provider)` calls `turn()` directly and collects events
   - Test 1: `headless_text_response` — MockProvider returns text, TurnEnd; verify TextChunk + TurnEnd received
   - Test 2: `headless_tool_events` — MockProvider returns tool_use; verify ToolStart + ToolComplete emitted
   - Test 3: `headless_provider_error` — MockErrorProvider returns Err; verify Error event + turn() returns Err
4. Ensure `cargo build --release` succeeds

## Dependencies
- Task 01: types.rs + ToolRegistry::with()
- Task 02: turn() function
- Task 03: middleware.rs + shell_hook_bridge()
- Task 04: session persistence for Conversation

## Implementation Approach
1. Write 3 failing noninteractive tests (TDD RED)
2. Rewrite main.rs with recipe pattern
3. Fix compilation issues
4. Make tests pass (GREEN)
5. Run release build + `./target/release/ap -p "What is 2+2?"` end-to-end

## Acceptance Criteria

1. **main.rs reads as a clean pipeline recipe**
   - Given the new main.rs
   - When reading the source
   - Then ToolRegistry, Middleware, Conversation, and dispatch are clearly visible as a recipe in main()

2. **Headless mode uses turn() directly**
   - Given a headless invocation via `run_headless`
   - When it processes a text response
   - Then it receives TurnEvent::TextChunk and prints to stdout, then TurnEvent::TurnEnd and exits 0

3. **Headless error path exits non-zero**
   - Given a MockErrorProvider that returns Err
   - When `run_headless_test` processes the error
   - Then `TurnEvent::Error` is received and `turn()` returns `Err`

4. **Headless tool events emitted**
   - Given a MockProvider that triggers a tool call
   - When `run_headless_test` collects events
   - Then `TurnEvent::ToolStart` and `TurnEvent::ToolComplete` are in the event list

5. **Release build succeeds**
   - Given the implementation is complete
   - When running `cargo build --release`
   - Then it compiles with zero warnings

6. **All tests pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all tests pass including the 3 rewritten noninteractive tests

## Metadata
- **Complexity**: High
- **Labels**: main, headless, pipeline, fp-refactor
- **Required Skills**: Rust, async/await, tokio, CLI
