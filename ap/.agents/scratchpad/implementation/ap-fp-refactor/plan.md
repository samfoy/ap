# Implementation Plan — ap FP Refactor

## Overview

Refactor `ap` from an OOP `AgentLoop` struct to a functional, pipeline-oriented
architecture. Core change: `AgentLoop` with mutable state → pure `turn()` function
with immutable `Conversation`. Shell hooks → composable `Middleware` chain.

**Constraint at every step:** `cargo build --release` must succeed, all existing
tests must pass (or be legitimately replaced). No orphaned code.

---

## Test Strategy

### Unit Tests

**types.rs** — `Conversation`, `TurnEvent`, `ToolCall`, `ToolMiddlewareResult`
- `conversation_new_has_empty_messages` — Conversation::new with id/model/config
- `conversation_with_user_message_appends` — immutable add returns new conv
- `turn_event_variants_are_clonable` — TextChunk, ToolStart, ToolComplete, TurnEnd, Error
- `tool_call_roundtrip_serde` — ToolCall serializes/deserializes
- `tool_middleware_result_variants` — Allow, Block, Transform variants

**tools/mod.rs** — `.with()` builder
- `registry_with_builder_chains_tools` — `.with(ReadTool).with(WriteTool)` registers 2
- `registry_with_builder_is_consuming` — returns `Self`, chainable
- `registry_with_defaults_still_works` — backwards compat preserved

**turn.rs** — pure `turn()` function
- `turn_text_only_response` — MockProvider returns text + TurnEnd → TurnEnd event emitted, Conversation has user+assistant msgs
- `turn_emits_text_chunks` — TextChunk events arrive in order
- `turn_with_tool_call` — MockProvider returns tool_use → tool executes → second LLM call with results
- `turn_provider_error_emits_error_event` — Err from provider → TurnEvent::Error + Err return
- `turn_pre_tool_block_skips_execution` — Middleware with blocking pre_tool stops execution, Block result returned to LLM
- `turn_pre_tool_transform_skips_execution` — Transform result returned without running tool
- `turn_pre_tool_allow_passes_through` — Allow(modified_call) used for execution

**middleware.rs** — `Middleware` chain
- `middleware_pre_tool_chain_allow_all` — 2 Allow middlewares in chain → tool executes
- `middleware_pre_tool_first_block_stops_chain` — first Block → subsequent middlewares not called
- `middleware_post_tool_transform_overrides_result` — post_tool Transform replaces tool result
- `middleware_pre_turn_chain_modifies_conv` — pre_turn closure mutates Conversation
- `shell_hook_bridge_noop_when_no_hooks` — empty HooksConfig → Allow for all calls
- `shell_hook_bridge_block_on_cancel` — mock a hook that returns Cancelled → Block

**session/mod.rs** — Conversation persistence
- `conversation_save_load_roundtrip` — save Conversation (with config), load it back
- `conversation_load_missing_config_uses_default` — old session JSON without config field loads OK
- `conversation_id_preserved` — id field survives round-trip

### Integration Tests

**tests/noninteractive.rs** (rewrite)
- `headless_text_response_printed_to_stdout` — run_headless with MockProvider → TurnEnd received, text events collected
- `headless_tool_call_emits_tool_events` — ToolStart + ToolComplete events emitted
- `headless_provider_error_returns_err` — MockErrorProvider → Error event emitted + error returned

**TUI tests** (update in-file tests in tui/mod.rs)
- Update all 7 tests from `UiEvent::*` → `TurnEvent::*`
- `tui_handles_text_chunk` — TurnEvent::TextChunk appends to conversation vec
- `tui_handles_tool_start` — tool_events vec updated
- `tui_handles_turn_end` — waiting state cleared

### E2E Scenario (Validator executes manually)

1. Build: `cd ap && cargo build --release`
2. Run headless with real Bedrock: `./target/release/ap -p "What is 1+1?"`
   - Expected: prints "2" (or similar), exits 0
3. Run TUI: `./target/release/ap`
   - Expected: 4-pane ratatui layout renders
   - Type `i` → Insert mode, type "list the files in current directory", Enter
   - Expected: tool calls shown, response streams in
4. Session round-trip: `./target/release/ap -s my-test-session -p "remember the number 42"`
   then: `./target/release/ap -s my-test-session -p "what number did I ask you to remember?"`
   - Expected: second call returns "42"
5. **Adversarial**: run with bad AWS creds via `AWS_ACCESS_KEY_ID=bad ap -p "hello"`
   - Expected: provider error, non-zero exit, clear error message
6. **Middleware path**: add a one-line pre_tool logger in main.rs, rebuild, run a tool call
   - Expected: log line appears on stderr

---

## Implementation Steps

### Step 1: Core types in `src/types.rs` + `ToolRegistry::with()` builder

**Files to create/modify:**
- `src/types.rs` — CREATE
- `src/tools/mod.rs` — ADD `.with()` builder
- `src/lib.rs` — ADD `pub mod types`

**What to implement:**
- `Conversation { id, model, messages, config }` with `Conversation::new()` and `Conversation::with_user_message()`
- `TurnEvent` enum (same variants as current `UiEvent`, just renamed):
  `TextChunk(String)`, `ToolStart { name, params }`, `ToolComplete { name, result }`, `TurnEnd`, `Error(String)`
- `ToolCall { id: String, name: String, params: serde_json::Value }`
- `ToolMiddlewareResult { Allow(ToolCall), Block(String), Transform(ToolResult) }`
- `Middleware { pre_turn, post_turn, pre_tool, post_tool }` struct with type aliases
- `ToolRegistry::with(self, tool: impl Tool + 'static) -> Self` (consuming builder)
- **DO NOT** touch `app.rs`, `main.rs`, `tui/`, or any existing tests

**Tests that pass after this step:**
- All new unit tests in `src/types.rs` (5 tests)
- `registry_with_builder_chains_tools`, `registry_with_builder_is_consuming`, `registry_with_defaults_still_works`
- All existing 80 tests still pass

**Demo:** `cargo test` shows 80+ tests all green; new type module compiles clean.

---

### Step 2: Pure `turn()` function in `src/turn.rs`

**Files to create/modify:**
- `src/turn.rs` — CREATE
- `src/lib.rs` — ADD `pub mod turn`

**What to implement:**
```rust
pub async fn turn(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
    tx: &tokio::sync::mpsc::Sender<TurnEvent>,
) -> anyhow::Result<Conversation>
```

Internal pipeline:
1. Apply `middleware.pre_turn` chain → (possibly modified) Conversation
2. Append user message (if not already in conv — conv passed in already has it via `with_user_message`)
3. `stream_completion` loop: stream events from provider → emit TextChunk, collect ToolUseStart/Params/End
4. Apply `middleware.post_turn` chain
5. `collect_tool_calls(pending)` → `Vec<ToolCall>`
6. For each ToolCall: run `middleware.pre_tool` → execute or skip → run `middleware.post_tool`
7. Emit ToolStart/ToolComplete events
8. `append_turn(conv, text, tool_results)` → new Conversation
9. If tool calls existed, loop (call provider again with results appended)
10. Emit `TurnEvent::TurnEnd`, return new Conversation

**Note on design:** `conv` is passed in with the user message already appended (via `Conversation::with_user_message()`). The caller in main.rs does:
```rust
let new_conv = turn(conv.with_user_message(input), &provider, &tools, &middleware, &tx).await?;
```

This keeps `turn()` pure (doesn't mutate, just transforms).

**Shell hooks integration:** `Middleware` struct is defined in types.rs. This step uses the `Middleware` type as defined — pre_tool/post_tool chains are `Vec<Box<dyn Fn(ToolCall) -> ToolMiddlewareResult + Send + Sync>>`. The shell bridge will come in Step 3. For now, tests use inline closures.

**Tests that pass after this step:**
- All `turn_*` tests (7 tests)
- All existing 80 tests still pass (app.rs still compiles and is still used)

**Demo:** `cargo test` shows 87+ tests green; `turn()` function works with MockProvider.

---

### Step 3: `Middleware` implementation in `src/middleware.rs` + shell hook bridge

**Files to create/modify:**
- `src/middleware.rs` — CREATE
- `src/lib.rs` — ADD `pub mod middleware`

**What to implement:**
- `Middleware::new()` constructor
- `Middleware::pre_tool(self, f: impl Fn(ToolCall) -> ToolMiddlewareResult + Send + Sync + 'static) -> Self` builder
- `Middleware::post_tool(self, f: ...)  -> Self` builder
- `Middleware::pre_turn(self, f: impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static) -> Self` builder
- `Middleware::post_turn(self, f: ...) -> Self` builder
- `pub fn shell_hook_bridge(config: &HooksConfig) -> Middleware` — wraps HookRunner:
  - If `config.pre_tool_call` set: add pre_tool middleware that calls `HookRunner::run_pre_tool_call`
  - Maps `HookOutcome::Cancelled` → `Block`, `Transformed` → `Transform`, others → `Allow`
  - If `config.post_tool_call` set: add post_tool middleware similarly
  - If `config.pre_turn` set: add pre_turn middleware (observer, returns None = no change)
  - If `config.post_turn` set: add post_turn middleware (observer, returns None = no change)

**Note:** `Middleware` struct is already defined in `types.rs`. `middleware.rs` adds:
- `impl Middleware { ... }` for builder methods
- `shell_hook_bridge()` free function

**Tests that pass after this step:**
- All `middleware_*` and `shell_hook_bridge_*` tests (6 tests)
- All existing + Step 1-2 tests still pass

**Demo:** `cargo test` shows 93+ tests green; middleware chain blocks/transforms tool calls correctly.

---

### Step 4: Update `src/session/mod.rs` to use `Conversation` as persistence type

**Files to modify:**
- `src/session/mod.rs` — UPDATE: add `Conversation` type or update `Session` to include `config`
- `src/session/store.rs` — UPDATE: add `save_conversation` / `load_conversation` methods

**Design choice:** Keep `Session` as-is for backward compat. Add parallel `save_conversation`/`load_conversation` methods to `SessionStore` that work with `Conversation`. When loading, missing `config` field defaults to `AppConfig::default()`. This avoids breaking existing session tests.

**What to implement:**
- `SessionStore::save_conversation(conv: &Conversation)` → saves as JSON using `conv.id` as filename
- `SessionStore::load_conversation(id: &str) -> Result<Conversation>` → loads, serde(default) on config
- Update `Conversation` in types.rs to derive `Serialize, Deserialize` (it should already)
- `AppConfig` must derive `Default` (check if it already does — it should from config.rs)

**Tests that pass after this step:**
- `conversation_save_load_roundtrip`
- `conversation_load_missing_config_uses_default`
- `conversation_id_preserved`
- All existing session tests still pass

**Demo:** `cargo test` shows 96+ tests green; Conversation round-trips through SessionStore.

---

### Step 5: Rewrite `src/main.rs` — recipe-style + headless using `turn()`

**Files to modify:**
- `src/main.rs` — REWRITE
- `tests/noninteractive.rs` — REWRITE

**What to implement:**

`main.rs` recipe pattern:
```rust
let tools = ToolRegistry::new()
    .with(ReadTool)
    .with(WriteTool)
    .with(EditTool)
    .with(BashTool);

let middleware = shell_hook_bridge(&config.hooks);  // from middleware.rs

let conv = match &args.session { ... }; // load or create Conversation

run_headless(conv, config, &provider, &tools, &middleware, prompt).await
// OR
run_tui(conv, config, &provider, tools, middleware).await
```

`run_headless` uses `turn()` directly:
```rust
let conv = conv.with_user_message(prompt.to_string());
let new_conv = turn(conv, provider, tools, middleware, &tx).await?;
// autosave if session id present
```

Drain loop reads `TurnEvent` (renamed from `UiEvent`).

**Important:** TUI still uses `AgentLoop` in this step (old code untouched). This compiles because both `AgentLoop` and `turn()` exist simultaneously. `run_tui` still calls the old path.

`tests/noninteractive.rs` rewrite:
- Import `ap::turn::turn` and `ap::types::{Conversation, TurnEvent}`
- `run_headless_test(prompt, provider)` helper that calls `turn()` directly
- 3 tests: text response, tool events, error path

**Tests that pass after this step:**
- All 3 rewritten noninteractive tests
- All existing app.rs tests still pass (AgentLoop still exists)
- TUI tests still pass (TuiApp still uses AgentLoop)

**Demo:** `cargo build --release` + `./target/release/ap -p "What is 2+2?"` works end-to-end.

---

### Step 6: Decouple TUI from `AgentLoop` — use `turn()` + `TurnEvent`

**Files to modify:**
- `src/tui/mod.rs` — REWRITE

**What to implement:**

New `TuiApp` fields:
```rust
pub struct TuiApp {
    // rendering state stays the same ...
    ui_rx: Option<mpsc::Receiver<TurnEvent>>,
    ui_tx: mpsc::Sender<TurnEvent>,
    conv: Arc<tokio::sync::Mutex<Conversation>>,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<Middleware>,
}
```

`TuiApp::new(ui_rx, ui_tx, conv, provider, tools, middleware, model_name)`

`handle_submit` spawns a task:
```rust
let conv = Arc::clone(&self.conv);
let provider = Arc::clone(&self.provider);
let tools = Arc::clone(&self.tools);
let middleware = Arc::clone(&self.middleware);
let tx = self.ui_tx.clone();
tokio::spawn(async move {
    let c = { conv.lock().await.clone().with_user_message(input) };
    match turn(c, &*provider, &*tools, &*middleware, &tx).await {
        Ok(new_conv) => { *conv.lock().await = new_conv; }
        Err(e) => { let _ = tx.send(TurnEvent::Error(e.to_string())).await; }
    }
});
```

`handle_ui_event` updated to match `TurnEvent` variants (same logic, just renamed).

Update `run_tui()` in `main.rs` to build `Arc<Middleware>`, `Arc<ToolRegistry>`, `Arc<Conversation>` and pass to new `TuiApp::new`.

All 7 TUI unit tests updated from `UiEvent::*` to `TurnEvent::*`.

**Tests that pass after this step:**
- All 7 TUI tests pass with TurnEvent
- All other tests pass
- `AgentLoop` is still present in `app.rs` (but no longer used by TUI)

**Demo:** `cargo build --release` + launch TUI (`./target/release/ap`) renders 4-pane layout and can submit a prompt.

---

### Step 7: Delete `src/app.rs` — remove `AgentLoop` and `UiEvent`

**Files to modify/delete:**
- `src/app.rs` — DELETE
- `src/lib.rs` — REMOVE `pub mod app` line
- Any remaining import of `ap::app::*` — clean up

**What to verify:**
- `grep -r "AgentLoop\|UiEvent\|app::Ui" ap/src ap/tests` → zero matches
- `cargo build --release` → zero warnings
- `cargo clippy -- -D warnings` → zero warnings
- All tests pass

**Tests:** All existing tests pass (app.rs tests are gone — they were replaced by turn.rs tests).

**Demo:** `cargo build --release` clean. `ap -p "hello"` works. `ap` TUI works.

---

### Step 8: README update — document Middleware API

**Files to modify:**
- `ap/README.md` — UPDATE

**Sections to add/update:**
- **Architecture** section: describe the `turn()` pipeline, `Conversation` immutability
- **Extending ap** section: show how to add a pre_tool middleware closure
- **Middleware** section: table of pre_turn/post_turn/pre_tool/post_tool with their signatures
- Update "Session Management" if anything changed
- Remove any references to `AgentLoop` or `UiEvent`

**Demo:** README accurately describes the new architecture.

---

## Success Criteria

- [ ] `cargo build --release` — zero warnings
- [ ] `cargo clippy -- -D warnings` — zero warnings
- [ ] `cargo test` — all tests pass (approximately 90+ after new tests added)
- [ ] `AgentLoop` struct is gone — `grep -r "AgentLoop" ap/src` → zero matches
- [ ] `UiEvent` is gone — `grep -r "UiEvent" ap/src ap/tests` → zero matches
- [ ] `app.rs` is deleted
- [ ] `Middleware` chain works — pre_tool closure blocks/allows/transforms correctly
- [ ] Shell hook config still works (bridge adapter verified by existing hook tests)
- [ ] `main.rs` reads as a clean pipeline setup (recipe-style)
- [ ] `ap -p "What is 2+2?"` works end-to-end with real Bedrock
- [ ] TUI renders and runs correctly
- [ ] Session round-trip works with `Conversation` type

## Step Wave Schedule

| Wave | Steps | Rationale |
|------|-------|-----------|
| 1 | Step 1 (types + builder) | Foundation — no existing code touched |
| 2 | Step 2 (turn.rs) | Core new function; AgentLoop still exists |
| 3 | Step 3 (middleware.rs) | Chain + bridge; tests in isolation |
| 4 | Step 4 (session update) | Persistence change; orthogonal |
| 5 | Step 5 (main.rs + headless) | Wire headless to turn(); TUI still old |
| 6 | Step 6 (TUI decouple) | Big TUI change; headless already verified |
| 7 | Step 7 (delete app.rs) | Cleanup; final compile verification |
| 8 | Step 8 (README) | Docs last |
