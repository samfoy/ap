# Research Context — ap FP Refactor

## Summary

The codebase is clean, well-tested (80 tests), and idiomatic Rust. The FP refactor has 4 major
structural changes: (1) `AgentLoop` → pure `turn()` fn, (2) `UiEvent` → `TurnEvent`,
(3) shell `HookRunner` → `Middleware` chain with shell-hook bridge, (4) `ToolRegistry` builder.

## Integration Points

### 1. TuiApp ↔ AgentLoop coupling (HIGH IMPACT)
- `TuiApp` stores `agent: Option<Arc<tokio::sync::Mutex<AgentLoop>>>` — direct struct coupling
- `TuiApp::new(ui_rx, agent_loop, model_name)` takes `AgentLoop` by value
- `handle_submit` calls `ag.run_turn(input)` via mutex lock in spawned task
- **After refactor**: TUI needs a different model. The `turn()` fn is not a method on a struct.
  Options:
  a) TUI stores `Arc<Mutex<Conversation>>` + provider/tools/middleware refs and calls `turn()` in spawn
  b) TUI stores a channel to send prompts to a background task that runs the turn loop
  c) TUI gets a `RunTurn` closure: `Box<dyn Fn(String, mpsc::Sender<TurnEvent>) -> ... >`
  **Recommendation**: TUI stores the `Conversation` in `Arc<Mutex<>>`, provider/tools/middleware as
  `Arc` refs, and spawns a task calling `turn(conv, provider, tools, middleware)`. The `conv` is
  updated after each turn.

### 2. UiEvent → TurnEvent (MEDIUM IMPACT)
- `UiEvent` used in: `app.rs` (emit), `main.rs` (headless drain), `tui/mod.rs` (handle_ui_event)
- 7 TUI tests reference `UiEvent::TextChunk`, `ToolStart`, `ToolComplete`, `TurnEnd`, `Error`
- `tests/noninteractive.rs` references `UiEvent` variants
- **After refactor**: Rename to `TurnEvent`, update all call sites. Tests need updating.
  The objective says merge UiEvent → TurnEvent — these are 1:1 variants, just renamed.

### 3. Middleware replacing HookRunner (MEDIUM IMPACT)
- `HookRunner` is sync (`std::process::Command`)
- Target `ToolMiddleware = Box<dyn Fn(ToolCall) -> ToolMiddlewareResult + Send + Sync>` is also sync — OK
- Target `TurnMiddleware = Box<dyn Fn(&Conversation) -> Option<Conversation> + Send + Sync>` is sync — OK
- Shell bridge: wrap `HookRunner::run_pre_tool_call` as `ToolMiddleware`:
  ```rust
  let runner = HookRunner::new(config.hooks.clone());
  middleware.pre_tool.push(Box::new(move |call| {
      match runner.run_pre_tool_call(&call.name, &call.params) {
          HookOutcome::Cancelled(reason) => ToolMiddlewareResult::Block(reason),
          _ => ToolMiddlewareResult::Allow(call),
      }
  }));
  ```
- Shell bridge for post_tool: `HookOutcome::Transformed(content)` → `ToolMiddlewareResult::Transform(ToolResult::ok(content))`
- Observer hooks (pre_turn, post_turn): wrap as `TurnMiddleware`
- **Note**: `ToolCall` needs to carry `id` (for ToolResult round-trip) + `name` + `params`

### 4. Session → Conversation (LOW IMPACT)
- `Session { id, created_at, model, messages }` — serialized to JSON
- Target `Conversation { id, model, messages, config }` — adds `config: AppConfig`
- Serialization: `config` field in Conversation JSON = new; sessions created before refactor won't have it
  → Add `#[serde(default)]` on `config` field to handle missing-field in old JSON
- The `created_at` field from Session is lost in Conversation definition in objective
  → Keep it as optional field `#[serde(skip_serializing_if = "Option::is_none")]` for backwards compat
  → OR keep Session as the serialization struct and map to/from Conversation
  **Recommendation**: Keep Session as the on-disk format (for backwards compat), and build Conversation
  from Session in main.rs. Session stays as-is in src/session/. This avoids migrating session files.
  OR rename Session → Conversation but keep same on-disk fields + add config. Either works.

### 5. ToolRegistry builder (LOW IMPACT)
- `ToolRegistry::new()` already exists
- Add `fn with(mut self, tool: impl Tool + 'static) -> Self` method
- Keep `with_defaults()` or remove it (main.rs explicitly lists tools)
- The objective shows `ToolRegistry::new().with(ReadTool)...` — chainable via consuming `self`

### 6. tests/noninteractive.rs (MEDIUM IMPACT)
- Creates `AgentLoop` directly with `MockProvider`
- Uses `UiEvent` variants
- Will need full rewrite to use `turn()` fn + `TurnEvent` + `Conversation`

## Key Design Constraints

### Constraint 1: TurnMiddleware can't be truly pure without async
The observer hooks (pre_turn/post_turn) in the current HookRunner write messages to a temp file
then run a shell command. This is synchronous but potentially slow. Since `TurnMiddleware` is
`Box<dyn Fn(&Conversation) -> Option<Conversation>>` (sync), this is fine — `std::process::Command`
is sync. No async needed for the middleware chain.

### Constraint 2: ToolCall needs `id` for ToolResult correlation
When executing tools, each tool call has an `id` (from the LLM) that must be returned in the
`ToolResult::tool_use_id`. The `ToolCall` struct must carry this `id`:
```rust
pub struct ToolCall {
    pub id: String,       // LLM-assigned tool use id
    pub name: String,
    pub params: serde_json::Value,
}
```

### Constraint 3: Middleware block/transform vs current HookOutcome
Current `HookOutcome` variants map to `ToolMiddlewareResult`:
- `Proceed` → `Allow(call)`
- `Cancelled(reason)` → `Block(reason)`
- `Transformed(content)` → `Transform(ToolResult::ok(content))`
- `HookWarning(msg)` → `Allow(call)` (non-fatal, log to stderr)
- `Observed` → `Allow(call)`

### Constraint 4: TUI tests use `UiEvent` directly
16 TUI tests + 3 noninteractive tests reference UiEvent. They must be updated when UiEvent
is renamed/replaced. The tests themselves are straightforward — just import TurnEvent instead.

### Constraint 5: Conversation vs Session on-disk format
The objective says "Update session persistence — save/load Conversation". Since Conversation adds
`config: AppConfig` over Session's fields, two approaches:
a) Make `Conversation` the on-disk type (new JSON schema); break old sessions
b) Keep `Session` on-disk, add serde(default) on config; back-compat
Given this is v0.1.0 / pre-release, approach (a) is fine — no users to break.

### Constraint 6: Shell hooks are synchronous — no async issue
The middleware chain is `Fn(ToolCall) -> ToolMiddlewareResult` (sync). The `turn()` fn is async
but can call sync middleware inline with `spawn_blocking` or just call them directly (they're fast
enough for shell invocations). Since these are sync closures called from async context with
`std::process::Command`, there's no stall issue for single-turn use.
→ Actually, blocking sync commands in async context IS a problem for tokio. Use `tokio::task::spawn_blocking`
  OR keep middleware as `async Fn`. The design shows sync signatures — let's call them with
  `spawn_blocking` at the boundary if needed, or document that heavy shell hooks should be async.
  For the shell hook bridge, the current code uses `std::process::Command` which blocks — same as
  before (was called from the async `run_turn` body). The existing code doesn't use spawn_blocking,
  so we can continue that pattern.

## Implementation Order (per objective)

1. `src/types.rs` — Conversation, TurnEvent, ToolCall, ToolMiddlewareResult, Middleware structs
2. `src/turn.rs` — pure `turn()` fn, delete AgentLoop
3. `src/middleware.rs` — Middleware struct + shell hook bridge
4. Update `src/tools/mod.rs` — add `.with()` builder
5. Update `src/main.rs` — recipe-style startup
6. Update `src/tui/` — wire to TurnEvent, decouple from AgentLoop
7. Update session persistence — save/load Conversation
8. Update non-interactive mode — use turn() pipeline
9. Tests + clippy
10. README update

## Files Touched (all changes)

| File | Change |
|------|--------|
| `src/app.rs` | DELETE (replaced by turn.rs) |
| `src/types.rs` | CREATE (Conversation, TurnEvent, ToolCall, ToolMiddlewareResult) |
| `src/turn.rs` | CREATE (pure turn() fn and helpers) |
| `src/middleware.rs` | CREATE (Middleware struct + shell bridge) |
| `src/tools/mod.rs` | ADD `.with()` builder method |
| `src/main.rs` | REWRITE (recipe-style) |
| `src/tui/mod.rs` | REWRITE (decouple from AgentLoop, use TurnEvent) |
| `src/session/mod.rs` | UPDATE (Conversation type or keep Session + bridge) |
| `src/lib.rs` | UPDATE (remove app mod, add types/turn/middleware) |
| `tests/noninteractive.rs` | REWRITE (use turn() + TurnEvent + Conversation) |
| `ap/README.md` | UPDATE (middleware API docs) |

## What NOT to Touch

- `src/provider/mod.rs` — Provider trait + Message types are fine
- `src/provider/bedrock.rs` — implementation unchanged
- `src/tools/bash.rs`, `read.rs`, `write.rs`, `edit.rs` — tool impls unchanged
- `src/config.rs` — config system unchanged
- `src/hooks/runner.rs` — keep as bridge target; used by middleware adapter
- `src/tui/events.rs` — keyboard event handling unchanged
- `src/tui/ui.rs` — rendering unchanged (uses TuiApp fields, not UiEvent directly)
- `src/session/store.rs` — SessionStore save/load logic unchanged (just new type to save)
