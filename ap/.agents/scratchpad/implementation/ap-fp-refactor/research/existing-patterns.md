# Existing Patterns — ap FP Refactor

## Code Organization
- Each module has a `mod.rs` barrel with pub re-exports (e.g., `hooks/mod.rs` re-exports `HookRunner`)
- `lib.rs` is minimal — 7 `pub mod` lines
- Doc comments on every public struct/fn/module with `//!` module-level docs
- Test modules use `#[cfg(test)] mod tests { ... }` directly inside the source file
- Integration tests in separate `tests/` directory (e.g., `tests/noninteractive.rs`)

## Error Handling
- `anyhow::Result<T>` for all fallible public functions
- `thiserror` for `ProviderError` (typed errors at boundary)
- Non-fatal: `eprintln!("ap: warning: ...")` + continue
- Fatal: `std::process::exit(1)` in `main.rs` only

## Async Patterns
- `tokio::spawn` for background agent tasks in TUI
- `mpsc::channel(256)` for agent → UI events
- `BoxFuture<'_, ToolResult>` for object-safe async tool trait
- `BoxStream<'a, Result<StreamEvent, ProviderError>>` for streaming provider
- `Arc<dyn Provider>` for shared provider reference

## Builder Patterns
- `ToolRegistry::new()` + `register(Box<dyn Tool>)` + `with_defaults()` convenience
- `AgentLoop::new()`, `with_session()`, `with_session_store()` construction variants
- `SessionStore::new()` (default path) vs `SessionStore::with_base(PathBuf)` (tests)

## Tool Trait
```rust
// src/tools/mod.rs:36-44
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;
    fn execute(&self, params: serde_json::Value) -> BoxFuture<'_, ToolResult>;
}
```

## Provider Trait
```rust
// src/provider/mod.rs:79-85
pub trait Provider: Send + Sync {
    fn stream_completion<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [serde_json::Value],
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>>;
}
```

## Hook System (current, sync)
- `HookRunner::run_pre_tool_call` → `HookOutcome::Proceed | Cancelled(reason) | HookWarning(msg)`
- `HookRunner::run_post_tool_call` → `HookOutcome::Transformed(content) | Observed | HookWarning(msg)`
- `HookRunner::run_observer_hook` → `HookOutcome::Observed | HookWarning(msg)`
- All use synchronous `std::process::Command` (no async needed)
- Pre-turn/post-turn pass messages JSON via AP_MESSAGES_FILE temp file

## Event Types
- `StreamEvent` — provider stream: TextDelta, ToolUseStart, ToolUseParams, ToolUseEnd, TurnEnd
- `UiEvent` — agent → UI: TextChunk, ToolStart, ToolComplete, TurnEnd, Error
- Target: `TurnEvent` replaces `UiEvent`

## Session / Persistence
- `Session { id, created_at, model, messages }` — serialize to JSON
- `SessionStore::save/load` writes `<base>/<id>.json`
- `AgentLoop::autosave_session` called at turn end (when `self.session.is_some()`)
- Session is opt-in (`--session <id>`) — ephemeral by default
- Target `Conversation` adds `config: AppConfig` field

## TUI Architecture
- `TuiApp` owns `agent: Option<Arc<tokio::sync::Mutex<AgentLoop>>>`
- `TuiApp::new(ui_rx, agent_loop, model_name)` takes the AgentLoop directly
- `handle_submit` calls `ag.run_turn(input)` in a spawned task via mutex lock
- `handle_ui_event` dispatches UiEvent variants to update conversation/tool_events state
- `TuiApp::headless()` constructor for tests (no terminal, no agent, no channel)
- 16 tests: 7 in `tui/mod.rs`, 9 in `tui/events.rs`

## Naming Conventions
- Snake_case everywhere
- Types in PascalCase
- Test function names: `verb_noun_condition` (e.g., `pre_tool_call_cancels_on_nonzero_exit`)
- Mock structs in tests: `MockProvider`, `MockErrorProvider`

## Current Test Count
- 80 tests total (all pass)
- app.rs: 3 tests
- hooks/runner.rs: ~8 tests (6 unit + 2 integration)
- tools/mod.rs + individual tools: ~20 tests
- session/: ~5 tests
- tui/: 16 tests
- config.rs: ~5 tests
- provider/mod.rs: ~3 tests
- tests/noninteractive.rs: 3 tests
