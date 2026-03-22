---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Hooks System

## Description
Implement `src/hooks/mod.rs` and `src/hooks/runner.rs` with a `HookRunner` that executes shell script hooks at lifecycle points. Hooks can cancel operations (pre_tool_call), transform results (post_tool_call), or simply observe (observer hooks). Missing scripts are non-fatal warnings.

## Background
Hooks are user-configured shell commands that fire at lifecycle events. The `pre_tool_call` hook is the most powerful — it can cancel a tool call by returning a non-zero exit code. `post_tool_call` can transform the result by writing to stdout. All other hooks (pre_turn, post_turn, on_error) are observer hooks — they receive data but cannot modify behavior. When a hook script is missing or not executable, the hook is skipped with a warning, never an error.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (Step 5 and test table)

**Note:** You MUST read the design document before beginning implementation. Section 4.7 covers the Hooks system and Appendix B has the full env var injection protocol.

## Technical Requirements
1. `src/hooks/mod.rs`:
   - `HookOutcome` enum: `Proceed`, `Cancelled(String)`, `Transformed(String)`, `Observed`, `HookWarning(String)`
   - Re-export `HookRunner`
2. `src/hooks/runner.rs` — `HookRunner`:
   - `new(config: HooksConfig) -> Self`
   - `run_pre_tool_call(&self, tool_name: &str, params: &serde_json::Value) -> HookOutcome`:
     - If no hook configured: `Proceed`
     - If hook script missing/not executable: `HookWarning("hook not found: <path>")`
     - Inject env vars: `AP_TOOL_NAME`, `AP_TOOL_PARAMS` (JSON string)
     - Run hook script; if exit 0: `Proceed`; if non-zero: `Cancelled(<stderr or "cancelled by hook">)`
   - `run_post_tool_call(&self, tool_name: &str, params: &serde_json::Value, result: &ToolResult) -> HookOutcome`:
     - If hook exits 0 with non-empty stdout: `Transformed(<stdout>)` (replaces result content)
     - If hook exits 0 with empty stdout: `Observed` (passthrough)
     - If non-zero exit: `HookWarning(<stderr>)` (result unchanged, warning logged)
     - Inject env vars: `AP_TOOL_NAME`, `AP_TOOL_PARAMS`, `AP_TOOL_RESULT` (JSON), `AP_TOOL_IS_ERROR`
   - `run_observer_hook(&self, hook_path: Option<&str>, env_vars: Vec<(String, String)>) -> HookOutcome`:
     - Used for pre_turn, post_turn, on_error
     - Always non-cancellable; non-zero exit → `HookWarning`
     - Temp file management: write message JSON to temp file, set `AP_MESSAGES_FILE` env var; delete temp file after hook completes
3. Hooks run via `std::process::Command::new("sh").arg("-c").arg(script_path)` (synchronous is fine for v1 — agent loop awaits the hook)

## Dependencies
- Task 01 (project scaffold) — `tempfile` crate declared
- Task 02 (config system) — `HooksConfig` struct
- Task 03 (tool trait) — `ToolResult` type

## Implementation Approach
1. Write all 6 unit tests using `#[cfg(test)]` — create actual temp `.sh` scripts in tests using `tempfile::NamedTempFile` with `#!/bin/sh` content
2. Implement `HookRunner` struct and `run_pre_tool_call` first (simplest)
3. Implement `run_post_tool_call`
4. Implement `run_observer_hook` with temp file management
5. Verify all 6 tests pass

## Acceptance Criteria

1. **pre_tool_call proceeds on exit 0**
   - Given a pre_tool_call hook script that exits 0
   - When `run_pre_tool_call("bash", &params)` is called
   - Then returns `HookOutcome::Proceed`

2. **pre_tool_call cancels on non-zero exit**
   - Given a pre_tool_call hook script that exits 1 with stderr `"blocked by policy"`
   - When `run_pre_tool_call("bash", &params)` is called
   - Then returns `HookOutcome::Cancelled(<message containing "blocked">)`

3. **post_tool_call transforms on non-empty stdout**
   - Given a post_tool_call hook script that echoes `"transformed result"` and exits 0
   - When `run_post_tool_call(...)` is called
   - Then returns `HookOutcome::Transformed("transformed result")`

4. **post_tool_call passthrough on empty stdout**
   - Given a post_tool_call hook script that produces no stdout and exits 0
   - When `run_post_tool_call(...)` is called
   - Then returns `HookOutcome::Observed`

5. **Observer hook warning on non-zero**
   - Given an observer hook (pre_turn) that exits 1
   - When `run_observer_hook(...)` is called
   - Then returns `HookOutcome::HookWarning(<message>)`

6. **Hook script not found → warning**
   - Given a hook path `/nonexistent/path/hook.sh`
   - When `run_pre_tool_call(...)` is called
   - Then returns `HookOutcome::HookWarning(<contains "not found">)`

7. **All 6 Hook Tests Pass**
   - Given the implementation is complete
   - When running `cargo test hooks`
   - Then all 6 hook tests pass

## Metadata
- **Complexity**: Medium
- **Labels**: hooks, shell, lifecycle, process
- **Required Skills**: Rust, std::process::Command, temp files
