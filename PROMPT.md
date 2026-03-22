# ap — FP Refactor

Refactor the existing `ap` Rust agent (in `ap/`) toward a functional, pipeline-oriented architecture. The goal is to make `ap` maximally hackable through direct code modification — no plugin system, just clean composable Rust that a developer can confidently fork and extend.

## Vision

`ap` should feel like a Unix pipeline in Rust. Each turn is a pure data transformation. Adding behavior means inserting a function into a chain, not wiring up an extension system. The codebase should be so clean and obvious that reading `app.rs` tells you the whole story.

## Core Refactoring Goals

### 1. Immutable Conversation State

Replace mutable agent loop state with an immutable `Conversation` value:

```rust
pub struct Conversation {
    pub id: String,
    pub model: String,
    pub messages: Vec<Message>,
    pub config: AppConfig,
}
```

Each turn returns a new `Conversation` — no mutation. The loop is:

```rust
loop {
    let (conv, events) = turn(conv, &provider, &tools, &middleware).await?;
    if is_done(&events) { break; }
}
```

### 2. Pipeline Turn Function

The core `turn` function is a pure async pipeline:

```rust
pub async fn turn(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
) -> Result<(Conversation, Vec<TurnEvent>)>
```

Internally it's a sequence of pure transforms:
1. `apply_pre_turn(conv, middleware)` → `Conversation`
2. `stream_completion(conv, provider)` → `Vec<TurnEvent>`
3. `collect_tool_calls(events)` → `Vec<ToolCall>`
4. `execute_tools(tool_calls, tools, middleware)` → `Vec<ToolResult>`
5. `append_turn(conv, events, results)` → `Conversation`

Each step is a standalone function, easily testable, easily replaced.

### 3. Rust Middleware Chain (replace shell hooks)

Replace the shell script hook system with a composable Rust middleware chain. Hooks become functions:

```rust
pub type ToolMiddleware = Box<dyn Fn(ToolCall) -> ToolMiddlewareResult + Send + Sync>;
pub type TurnMiddleware = Box<dyn Fn(&Conversation) -> Option<Conversation> + Send + Sync>;

pub struct Middleware {
    pub pre_turn: Vec<TurnMiddleware>,
    pub post_turn: Vec<TurnMiddleware>,
    pub pre_tool: Vec<ToolMiddleware>,
    pub post_tool: Vec<ToolMiddleware>,
}
```

`ToolMiddlewareResult`:
```rust
pub enum ToolMiddlewareResult {
    Allow(ToolCall),              // pass through (possibly modified)
    Block(String),                // cancel with reason sent to Claude
    Transform(ToolResult),        // skip execution, return this result
}
```

Someone who wants "log every bash call" just does:
```rust
middleware.pre_tool.push(Box::new(|call| {
    if call.name == "bash" { eprintln!("[tool] bash: {}", call.params); }
    ToolMiddlewareResult::Allow(call)
}));
```

**Keep shell hook config for backwards compat** — `HooksConfig` shell commands get wrapped as `ToolMiddleware`/`TurnMiddleware` automatically at startup if configured. But the primary API is Rust closures.

### 4. Tool Registry as Data

Tools are already trait objects — keep that. But make `ToolRegistry` more functional:
- `ToolRegistry::with(tool)` builder pattern (chainable)
- Tools registered in `main.rs` in one obvious place, no magic discovery

### 5. Clean `main.rs`

`main.rs` should read like a recipe:
```rust
let tools = ToolRegistry::new()
    .with(ReadTool)
    .with(WriteTool)
    .with(EditTool)
    .with(BashTool);

let middleware = Middleware::new()
    .pre_tool(log_tool_calls)    // built-in logger
    .pre_tool(shell_hook_bridge(&config.hooks)); // shell hook compat

let conv = Conversation::new(session_id, config.provider.model.clone(), config.clone());

run(conv, &provider, &tools, &middleware).await?;
```

## What to Keep

- `Tool` trait and all 4 built-in tools — they're fine
- `Provider` trait and `BedrockProvider` — good abstraction
- ratatui TUI — keep, wire to new event types
- Config system — keep
- Session persistence — update to save/load `Conversation`
- `-p` non-interactive mode — keep

## What to Remove/Replace

- `AgentLoop` struct with mutable state → replace with pure `turn()` function
- Shell-only hook system → replace with `Middleware` chain (shell hooks become an adapter)
- `UiEvent` enum → merge into `TurnEvent` (single event type for both TUI and headless)

## Implementation Plan

Implement in order — each step must compile and tests must pass:

1. **Define core types** — `Conversation`, `TurnEvent`, `ToolCall`, `ToolMiddlewareResult`, `Middleware` structs in `src/types.rs`
2. **Refactor agent loop** — implement pure `turn()` in `src/turn.rs`, delete `AgentLoop`
3. **Implement Middleware chain** — `src/middleware.rs` with `Middleware` struct and shell hook bridge
4. **Wire ToolRegistry builder** — chainable `.with()` pattern
5. **Update main.rs** — clean recipe-style startup
6. **Update TUI** — wire to new `TurnEvent` type
7. **Update session persistence** — save/load `Conversation`
8. **Update non-interactive mode** — use new `turn()` pipeline
9. **Tests + clippy** — all tests pass, zero warnings
10. **README update** — document the middleware API and how to extend

## Acceptance Criteria

- [ ] `cargo build --release` — zero warnings
- [ ] `ap -p "read Cargo.toml and summarize it"` — works end-to-end
- [ ] TUI renders and runs correctly
- [ ] `AgentLoop` struct is gone — replaced by pure `turn()` function
- [ ] `Middleware` chain works — pre_tool closure can block/allow/transform a tool call
- [ ] Shell hook config still works (bridge adapter)
- [ ] `main.rs` reads clearly as a pipeline setup
- [ ] All tests pass
- [ ] Output LOOP_COMPLETE when done

## Notes

- This is a refactor — behavior must not change, only structure
- Commit frequently with conventional commits
- If a step reveals the design needs adjustment, update the scratchpad and continue
- The middleware chain is the main new concept — get that right first
