---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: ShellTool Implementation

## Description
Create `ap/src/tools/shell.rs` implementing the `Tool` trait for shell-based tools discovered from TOML files. `ShellTool` wraps a `DiscoveredTool`, generates a JSON schema from its params, and executes the tool command via `sh -c` with `AP_PARAM_*` env vars injected. Export `ShellTool` from `ap/src/tools/mod.rs`.

## Background
`ShellTool` is the bridge between the discovery system and Claude's tool API. It follows the same execution model as the existing `BashTool` (same output format, same `sh -c` invocation), but adds parameter validation and env var injection. The `root: PathBuf` field ensures commands run in the project root directory.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/tool-discovery/design.md` (Section 3.2)

**Additional References:**
- `.agents/scratchpad/implementation/tool-discovery/context.md` (codebase patterns)
- `ap/src/tools/mod.rs` — existing Tool trait + BashTool for reference
- `.agents/scratchpad/implementation/tool-discovery/plan.md` (Step 3 implementation notes)

**Note:** You MUST read the design document before beginning implementation. Read `ap/src/tools/mod.rs` to understand the `Tool` trait contract before implementing.

## Technical Requirements
1. Create `ap/src/tools/shell.rs` with:
   ```rust
   pub struct ShellTool { tool: DiscoveredTool, root: PathBuf }
   impl ShellTool { pub fn new(tool: DiscoveredTool, root: PathBuf) -> Self }
   ```
2. Implement `Tool` trait:
   - `fn name(&self) -> &str` → `&self.tool.name`
   - `fn description(&self) -> &str` → `&self.tool.description`
   - `fn schema(&self) -> serde_json::Value` → JSON schema (see design Section 3.2)
   - `fn execute(&self, params: serde_json::Value) -> BoxFuture<'_, ToolResult>` → validate + spawn
3. Schema generation: iterate `tool.params`, build `properties` object, collect keys where `required == true` into `"required"` array
4. Execution contract (in order):
   a. For each `required: true` param: if missing from `params` JSON → return `ToolResult::err("missing required parameter: {key}")`
   b. Collect env vars: `AP_PARAM_{KEY_UPPERCASE}` = value for each supplied param
   c. Spawn: `std::process::Command::new("sh").arg("-c").arg(&self.tool.command).envs(&env_vars).current_dir(&self.root).output()`
   d. On spawn error → `ToolResult::err("failed to spawn command: {e}")`
   e. On success → `ToolResult::ok("{stdout}\n{stderr}\nexit: {code}")` (same format as BashTool)
5. `Box::pin(async move { ... })` wrapping the synchronous `Command::output()` call
6. No `unwrap()` or `expect()` outside test modules
7. Add `pub mod shell;` and `pub use shell::ShellTool;` to `ap/src/tools/mod.rs`

## Dependencies
- Task 01: Discovery types must exist (`DiscoveredTool`, `ParamSpec`)

## Implementation Approach
1. Read `ap/src/tools/mod.rs` to understand the `Tool` trait and `ToolResult` type
2. Write failing tests for schema generation and execution (RED)
3. Implement `ShellTool` struct, `new()`, and `Tool` impl (GREEN)
4. Run `cargo test --package ap tools::shell::` — all pass (REFACTOR)
5. Run `cargo check --package ap` — clean

## Acceptance Criteria

1. **Schema required params in required array**
   - Given a `DiscoveredTool` with params: `user` (required=true) and `format` (required=true), `verbose` (required=false)
   - When `schema()` is called
   - Then `schema["input_schema"]["required"]` contains `["user", "format"]` but NOT `"verbose"`

2. **Schema optional params in properties but not required**
   - Given a `DiscoveredTool` with one `required = false` param `"verbose"`
   - When `schema()` is called
   - Then `schema["input_schema"]["properties"]["verbose"]` exists but `"verbose"` is not in `schema["input_schema"]["required"]`

3. **Execute with all required params succeeds**
   - Given a `ShellTool` with command `echo $AP_PARAM_FOO` and required param `foo`
   - When `execute(json!({"foo": "bar"}))` is called
   - Then `ToolResult` is ok and content contains `"bar"`

4. **Env var key is uppercased**
   - Given a `ShellTool` with command `echo $AP_PARAM_MY_KEY` and required param `my_key`
   - When `execute(json!({"my_key": "hello"}))` is called
   - Then `ToolResult` is ok and content contains `"hello"`

5. **Missing required param returns error**
   - Given a `ShellTool` with required param `foo`, and `execute` called with `json!({})`
   - When `execute(json!({}))` is called
   - Then `ToolResult` is an error containing `"missing required parameter: foo"`

6. **Optional param absent runs successfully**
   - Given a `ShellTool` with command `echo ${AP_PARAM_OPT:-default}` and optional param `opt`
   - When `execute(json!({}))` is called (no `opt` provided)
   - Then `ToolResult` is ok and content contains `"default"`

7. **Non-zero exit code is not an error**
   - Given a `ShellTool` with command `exit 1` (no params)
   - When `execute(json!({}))` is called
   - Then `ToolResult` is ok (not error), content contains `"exit: 1"`

8. **Command spawn failure returns error**
   - Given a `ShellTool` whose command is something that causes a spawn failure
   - When `execute(json!({}))` is called
   - Then `ToolResult` is an error containing `"failed to spawn"`

9. **Command runs in root dir**
   - Given a `ShellTool` with command `pwd` and `root` set to a temp dir path
   - When `execute(json!({}))` is called
   - Then content contains the temp dir path

10. **Unit Tests Pass**
    - Given the implementation is complete
    - When running `cargo test --package ap tools::shell::`
    - Then all 9 shell tool tests pass and `cargo check` is clean

## Metadata
- **Complexity**: Medium
- **Labels**: tools, shell, execution, rust
- **Required Skills**: Rust, async, process execution, JSON schema
