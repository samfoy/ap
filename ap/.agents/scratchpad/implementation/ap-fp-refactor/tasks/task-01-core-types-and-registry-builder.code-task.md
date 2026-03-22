---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Core Types in `src/types.rs` + `ToolRegistry::with()` Builder

## Description
Define the foundational data types for the FP refactor — `Conversation`, `TurnEvent`, `ToolCall`, `ToolMiddlewareResult`, and `Middleware` — in a new `src/types.rs` module. Add a chainable `.with()` builder to `ToolRegistry`. No existing code is touched; this step is purely additive.

## Background
The FP refactor replaces the mutable `AgentLoop` struct with an immutable `Conversation` value and a pure `turn()` function. This step establishes all the data types those future steps will use. `AgentLoop` continues to exist and all existing 80 tests continue to pass.

## Reference Documentation
**Required:**
- Design/Plan: ap/.agents/scratchpad/implementation/ap-fp-refactor/plan.md

**Additional References:**
- ap/.agents/scratchpad/implementation/ap-fp-refactor/context.md (codebase patterns)

**Note:** You MUST read the plan document before beginning implementation.

## Technical Requirements
1. Create `src/types.rs` with:
   - `Conversation { id: String, model: String, messages: Vec<Message>, config: AppConfig }` — derive `Clone, Debug, Serialize, Deserialize`
   - `Conversation::new(id: impl Into<String>, model: impl Into<String>, config: AppConfig) -> Self`
   - `Conversation::with_user_message(self, content: impl Into<String>) -> Self` — immutable add, returns new Conversation
   - `TurnEvent` enum: `TextChunk(String)`, `ToolStart { name: String, params: serde_json::Value }`, `ToolComplete { name: String, result: String }`, `TurnEnd`, `Error(String)` — derive `Clone, Debug`
   - `ToolCall { id: String, name: String, params: serde_json::Value }` — derive `Clone, Debug, Serialize, Deserialize`
   - `ToolMiddlewareResult` enum: `Allow(ToolCall)`, `Block(String)`, `Transform(ToolResult)` — derive `Debug`
   - Type aliases: `type ToolMiddlewareFn = Box<dyn Fn(ToolCall) -> ToolMiddlewareResult + Send + Sync>`, `type TurnMiddlewareFn = Box<dyn Fn(&Conversation) -> Option<Conversation> + Send + Sync>`
   - `Middleware { pre_turn: Vec<TurnMiddlewareFn>, post_turn: Vec<TurnMiddlewareFn>, pre_tool: Vec<ToolMiddlewareFn>, post_tool: Vec<ToolMiddlewareFn> }` — derive nothing (closures can't derive), add `impl Default for Middleware`
2. Add to `src/tools/mod.rs`: `ToolRegistry::with(self, tool: impl Tool + 'static) -> Self` consuming builder method
3. Add `pub mod types;` to `src/lib.rs`
4. Unit tests inside `src/types.rs` (in `#[cfg(test)]` module) covering all 5 required test cases
5. Unit tests inside `src/tools/mod.rs` covering the 3 builder tests
6. DO NOT touch `app.rs`, `main.rs`, `tui/`, `hooks/`, or any existing tests

## Dependencies
- No task dependencies — this is Step 1

## Implementation Approach
1. Write failing tests first (TDD RED)
2. Implement types.rs with all structs/enums
3. Add `.with()` to ToolRegistry
4. Ensure all existing 80 tests still pass (no regressions)

## Acceptance Criteria

1. **Conversation type is immutable-friendly**
   - Given a `Conversation` with empty messages
   - When calling `conversation.with_user_message("hello")`
   - Then a new `Conversation` is returned with one user message appended, and the original is unchanged

2. **TurnEvent variants are clonable**
   - Given each `TurnEvent` variant (TextChunk, ToolStart, ToolComplete, TurnEnd, Error)
   - When cloning them
   - Then no compile error occurs and cloned values equal originals

3. **ToolCall roundtrips through serde**
   - Given a `ToolCall { id: "1", name: "bash", params: json!({"cmd": "ls"}) }`
   - When serializing to JSON and deserializing back
   - Then the resulting ToolCall equals the original

4. **ToolMiddlewareResult has all three variants**
   - Given `Allow`, `Block`, and `Transform` variants
   - When pattern-matching on each
   - Then all three arms compile and match correctly

5. **ToolRegistry `.with()` builder chains**
   - Given `ToolRegistry::new().with(ReadTool).with(WriteTool)`
   - When calling `registry.tool_schemas().len()`
   - Then the result is 2

6. **ToolRegistry `.with_defaults()` still works**
   - Given calling `ToolRegistry::with_defaults()`
   - When checking the registered tool count
   - Then it equals 4 (unchanged from before)

7. **All existing tests pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all 80+ tests pass with zero failures

## Metadata
- **Complexity**: Low
- **Labels**: types, foundation, fp-refactor
- **Required Skills**: Rust, serde
