---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: main.rs Recipe-Style + Headless Mode

## Description

Rewrite `src/main.rs` to use the new FP pipeline: `Conversation`, `turn()`, `Middleware`,
and `ToolRegistry::with()`. Remove all references to `AgentLoop` and `UiEvent` from `main.rs`
and `tests/noninteractive.rs`. The headless runner drives the `turn()` loop directly and drains
`TurnEvent` from the mpsc channel. `AgentLoop` must still compile (it is deleted in Step 7),
but `main.rs` must not use it.

## Background

Steps 1–4 created `types.rs` (Conversation, TurnEvent, Middleware), `turn.rs` (pure turn()),
`middleware.rs` (builder + shell_hook_bridge), and `session/store.rs`
(save_conversation/load_conversation). This step wires them together in `main.rs` and updates
the integration tests to use `TurnEvent` instead of `UiEvent`.

Key interfaces to use:
- `Conversation::new(id, model, config)` + `with_user_message(prompt)`
- `turn(conv, &provider, &tools, &middleware, &tx) -> Result<Conversation>`
- `Middleware::new().pre_tool(...).pre_turn(...)`  — builder pattern
- `shell_hook_bridge(&config.hooks) -> Middleware`
- `SessionStore::save_conversation` / `load_conversation`
- `TurnEvent::{TextChunk, ToolStart, ToolComplete, TurnEnd, Error}`

The TUI (`run_tui`) still uses `AgentLoop` in this step — TUI decoupling is Step 6.
Only `run_headless` and session plumbing need updating here.

## Reference Documentation

**Required:**
- Design: `.agents/scratchpad/implementation/ap-fp-refactor/` (types.rs, turn.rs, middleware.rs source)

**Additional References:**
- `src/types.rs` — Conversation, TurnEvent, Middleware definitions
- `src/turn.rs` — turn() signature
- `src/middleware.rs` — Middleware builder + shell_hook_bridge
- `src/session/store.rs` — save_conversation / load_conversation
- `tests/noninteractive.rs` — current tests using UiEvent (must be updated)

**Note:** Read the source files listed above before beginning implementation.

## Technical Requirements

1. `main.rs::run_headless` must use `turn()` + `TurnEvent` instead of `AgentLoop` + `UiEvent`
2. Session loading uses `SessionStore::load_conversation` when `--session <id>` is given
3. Session saving uses `SessionStore::save_conversation` after each turn (when `--session` set)
4. `run_tui` continues using `AgentLoop` — no change here (Step 6 handles TUI)
5. `tests/noninteractive.rs` must be rewritten to use `TurnEvent` instead of `UiEvent`
6. `main.rs` must not import `app::AgentLoop` or `app::UiEvent` from headless path (run_tui may still)
7. `Middleware` is built with `shell_hook_bridge` if hooks are configured
8. `cargo build --release` — zero warnings
9. All existing tests pass (≥105) plus 3 updated noninteractive tests

## Dependencies

- Step 1 (types.rs + ToolRegistry::with()) — complete ✓
- Step 2 (turn.rs pure turn()) — complete ✓
- Step 3 (middleware.rs + shell_hook_bridge) — complete ✓
- Step 4 (save_conversation / load_conversation) — complete ✓

## Implementation Approach

1. Read `src/main.rs`, `src/turn.rs`, `src/types.rs`, `src/middleware.rs`, `src/session/store.rs`
2. Rewrite `run_headless` in `main.rs`:
   - Build `Middleware::new()` and call `shell_hook_bridge(&config.hooks)` to extend it
   - Load session as `Conversation` via `load_conversation` (or create fresh)
   - Drive loop: `conv = turn(conv.with_user_message(prompt), &provider, &tools, &middleware, &tx).await?`
   - Drain `TurnEvent` from `rx`, print text / log tool calls / handle errors
   - After loop, save conv via `save_conversation` if session id is set
3. Rewrite `tests/noninteractive.rs`:
   - Replace `AgentLoop`/`UiEvent` imports with `ap::turn::turn`, `ap::types::{Conversation, TurnEvent}`
   - Helper `run_headless_test` creates `Conversation`, calls `turn()`, drains `TurnEvent`
   - Update all 3 tests to match on `TurnEvent` variants
4. Verify `cargo test` passes, `cargo clippy -- -D warnings` clean

## Acceptance Criteria

1. **run_headless uses turn() pipeline**
   - Given `run_headless` is called with a mock prompt
   - When `main.rs` compiles
   - Then it imports `turn::turn` and `types::TurnEvent`, not `app::AgentLoop`

2. **TurnEvent::TextChunk received in headless**
   - Given a `MockProvider` emitting `TextDelta("Hello")` + `TurnEnd`
   - When `run_headless_test("test", provider)` is called in tests
   - Then the returned events contain `TurnEvent::TextChunk("Hello")`

3. **TurnEvent::Error emitted on provider failure**
   - Given a `MockErrorProvider` returning a `ProviderError`
   - When `turn()` is called
   - Then `TurnEvent::Error(msg)` is sent on the channel and the function returns `Err`

4. **Session save/load wired**
   - Given `--session foo` is passed
   - When headless mode runs successfully
   - Then `save_conversation` is called with the updated `Conversation`

5. **Middleware shell_hook_bridge integrated**
   - Given `config.hooks` is non-empty
   - When `run_headless` builds `Middleware`
   - Then `shell_hook_bridge(&config.hooks)` is called and the result chained

6. **All noninteractive tests updated**
   - Given `tests/noninteractive.rs` uses `TurnEvent`
   - When `cargo test` runs
   - Then all 3 noninteractive tests pass with zero references to `UiEvent`

7. **Build and test green**
   - Given the implementation is complete
   - When `cargo build --release && cargo test && cargo clippy -- -D warnings` runs
   - Then zero errors, zero warnings, ≥105 tests pass

## Metadata
- **Complexity**: Medium
- **Labels**: main, headless, session, turevent, refactor
- **Required Skills**: Rust async, tokio mpsc, Bedrock provider pattern
