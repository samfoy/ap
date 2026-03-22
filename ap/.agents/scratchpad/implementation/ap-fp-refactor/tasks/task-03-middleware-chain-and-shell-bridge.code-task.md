---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Middleware Chain + Shell Hook Bridge in `src/middleware.rs`

## Description
Implement the `Middleware` builder API and shell hook bridge in a new `src/middleware.rs`. The `Middleware` struct is already defined in `types.rs`; this step adds `impl Middleware` builder methods and the `shell_hook_bridge()` function that wraps `HooksConfig` shell commands as middleware closures.

## Background
The `Middleware` struct is the primary extension point for ap. Users add closures to pre_tool/post_tool/pre_turn/post_turn chains. The shell hook bridge ensures backwards compatibility — existing `ap.toml` `[hooks]` configurations continue to work by being automatically adapted into middleware closures at startup.

## Reference Documentation
**Required:**
- Design/Plan: ap/.agents/scratchpad/implementation/ap-fp-refactor/plan.md

**Additional References:**
- ap/.agents/scratchpad/implementation/ap-fp-refactor/context.md (codebase patterns)
- ap/src/hooks/runner.rs (HookRunner implementation to bridge)

**Note:** You MUST read the plan document before beginning implementation. Pay particular attention to the Step 3 section describing the shell hook bridge mapping.

## Technical Requirements
1. Create `src/middleware.rs` with `impl Middleware`:
   - `Middleware::new() -> Self` constructor (same as `Default::default()`)
   - `Middleware::pre_tool(self, f: impl Fn(ToolCall) -> ToolMiddlewareResult + Send + Sync + 'static) -> Self` consuming builder
   - `Middleware::post_tool(self, f: ...) -> Self` consuming builder
   - `Middleware::pre_turn(self, f: impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static) -> Self` consuming builder
   - `Middleware::post_turn(self, f: ...) -> Self` consuming builder
2. `pub fn shell_hook_bridge(config: &HooksConfig) -> Middleware`:
   - If `config.pre_tool_call` is `Some`: add pre_tool middleware that calls `HookRunner::run_pre_tool_call` and maps `HookOutcome::Cancelled → Block`, `HookOutcome::Transformed(content) → Transform(ToolResult::ok(content))`, anything else → `Allow(call)`
   - If `config.post_tool_call` is `Some`: add post_tool middleware similarly
   - If `config.pre_turn` is `Some`: add pre_turn middleware (observer, always returns `None`)
   - If `config.post_turn` is `Some`: add post_turn middleware (observer, always returns `None`)
   - If no hooks configured, returns `Middleware::new()` (empty, no-op)
3. Add `pub mod middleware;` to `src/lib.rs`
4. Unit tests inside `src/middleware.rs` (#[cfg(test)]) covering all 6 test cases

## Dependencies
- Task 01: `Middleware` struct defined in `src/types.rs`
- Task 02: `turn()` function uses `Middleware` (tests can reference turn.rs behavior)

## Implementation Approach
1. Write failing tests (TDD RED)
2. Implement builder methods on Middleware
3. Implement shell_hook_bridge (may need to mock HookRunner in tests or test with empty config)
4. Run full suite — all existing tests must pass

## Acceptance Criteria

1. **Pre-tool chain: all Allow → tool executes**
   - Given a Middleware with two pre_tool closures, both returning `Allow(call)`
   - When running the pre_tool chain on a ToolCall
   - Then the final result is `Allow` and both closures are called

2. **Pre-tool chain: first Block stops the chain**
   - Given a Middleware with two pre_tool closures, first returns `Block("stop")`, second returns `Allow(call)`
   - When running the pre_tool chain
   - Then the result is `Block("stop")` and the second closure is NOT called

3. **Post-tool Transform overrides result**
   - Given a Middleware with one post_tool closure that returns `Transform(ToolResult::ok("override"))`
   - When running the post_tool chain on a completed tool result
   - Then the final result contains "override"

4. **Pre-turn chain modifies Conversation**
   - Given a Middleware with one pre_turn closure that adds a system note to the Conversation
   - When running the pre_turn chain on a Conversation
   - Then the returned Conversation reflects the modification

5. **shell_hook_bridge: no-op for empty HooksConfig**
   - Given a `HooksConfig { pre_tool_call: None, post_tool_call: None, pre_turn: None, post_turn: None }`
   - When calling `shell_hook_bridge(&config)`
   - Then the returned Middleware has empty pre_tool and post_tool chains (Allow for all)

6. **shell_hook_bridge: Block on HookOutcome::Cancelled**
   - Given a HooksConfig with a pre_tool_call hook script that produces a Cancelled outcome (e.g., exits non-zero in a way HookRunner interprets as Cancelled)
   - When the pre_tool bridge runs
   - Then `ToolMiddlewareResult::Block` is returned

7. **All existing tests pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all tests pass with zero failures

## Metadata
- **Complexity**: Medium
- **Labels**: middleware, hooks, bridge, fp-refactor
- **Required Skills**: Rust, closures, shell interop
