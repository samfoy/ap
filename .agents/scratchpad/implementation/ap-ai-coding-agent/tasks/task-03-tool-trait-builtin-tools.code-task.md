---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Tool Trait + 4 Built-in Tools

## Description
Implement the `Tool` trait, `ToolResult`, `ToolRegistry`, and all four built-in tools: `read`, `write`, `edit`, and `bash`. Each tool has a JSON schema, is object-safe via `BoxFuture`, and has full unit test coverage (12 tests total).

## Background
Tools are the core action primitives that Claude uses to interact with the filesystem and shell. The `Tool` trait must be object-safe so tools can be stored as `Box<dyn Tool>` in the registry. `EditTool` has special behavior: if `old_text` matches more than once, it returns an error with the count (never a silent replacement). `BashTool` has no timeout in v1.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (test table in Section 1)

**Note:** You MUST read the design document before beginning implementation. Sections 4.3 (Tool trait) and 4.5 (built-in tools) are essential.

## Technical Requirements
1. `src/tools/mod.rs`:
   - `ToolResult { content: String, is_error: bool }` — derives Serialize/Deserialize
   - `Tool` trait: `name(&self) -> &str`, `description(&self) -> &str`, `schema(&self) -> serde_json::Value`, `execute(&self, params: serde_json::Value) -> BoxFuture<'_, ToolResult>` — object-safe
   - `ToolRegistry { tools: Vec<Box<dyn Tool>> }` with:
     - `new() -> Self` — empty
     - `register(tool: Box<dyn Tool>)`
     - `find_by_name(name: &str) -> Option<&dyn Tool>`
     - `all_schemas() -> Vec<serde_json::Value>` — returns all tool schemas
     - `with_defaults() -> Self` — pre-populated with the 4 built-ins
2. `src/tools/read.rs` — `ReadTool`:
   - Params: `{ path: String }`
   - Returns file contents as string; if binary/unreadable, is_error=true with message
   - Schema: JSON Schema object with `path` as required string property
3. `src/tools/write.rs` — `WriteTool`:
   - Params: `{ path: String, content: String }`
   - Creates parent directories if needed; overwrites existing files
   - Returns confirmation message on success
4. `src/tools/edit.rs` — `EditTool`:
   - Params: `{ path: String, old_text: String, new_text: String }`
   - Reads file, counts occurrences of `old_text`
   - If 0 occurrences: is_error=true, "old_text not found in file"
   - If >1 occurrences: is_error=true, "old_text matches N occurrences (must be unique)"
   - If exactly 1: replace and write back; return confirmation
5. `src/tools/bash.rs` — `BashTool`:
   - Params: `{ command: String }`
   - Runs via `sh -c <command>`, captures stdout, stderr, exit code
   - Output format: `"{stdout}\n{stderr}\nexit: {code}"`
   - No timeout in v1; no sandboxing
   - `is_error: false` always (even non-zero exit is captured, not an error at tool level)

## Dependencies
- Task 01 (project scaffold) — `Cargo.toml` declares `futures`, `tokio`, `serde_json`

## Implementation Approach
1. Write all 12 unit tests first (RED phase) using `#[cfg(test)]` in each tool file
2. Implement `Tool` trait and `ToolResult` in `mod.rs`
3. Implement each tool one at a time: write test → implement → pass → next tool
4. Implement `ToolRegistry::with_defaults()` last, after all 4 tools pass their tests

## Acceptance Criteria

1. **ReadTool: reads existing file**
   - Given a file at `/tmp/ap-test-read.txt` containing `"hello"`
   - When `ReadTool.execute({ "path": "/tmp/ap-test-read.txt" })` is called
   - Then `ToolResult { content: "hello", is_error: false }` is returned

2. **ReadTool: missing file is_error**
   - Given no file at `/tmp/ap-nonexistent-xyz.txt`
   - When `ReadTool.execute({ "path": "/tmp/ap-nonexistent-xyz.txt" })` is called
   - Then `ToolResult { is_error: true, content: <contains "not found" or similar> }`

3. **EditTool: multiple matches returns error with count**
   - Given a file containing `"foo foo foo"`
   - When `EditTool.execute({ "path": ..., "old_text": "foo", "new_text": "bar" })` is called
   - Then `ToolResult { is_error: true, content: <contains "3 occurrences"> }`

4. **WriteTool: creates parent directories**
   - Given path `/tmp/ap-test-nested/deep/dir/file.txt`
   - When `WriteTool.execute({ "path": ..., "content": "hi" })` is called
   - Then the file exists at that path and contains `"hi"`

5. **BashTool: captures stdout, stderr, exit code**
   - Given `command = "echo out; echo err >&2; exit 42"`
   - When `BashTool.execute` is called
   - Then content contains `"out"`, `"err"`, and `"exit: 42"`; `is_error: false`

6. **All 12 Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test tools`
   - Then all 12 tool tests pass with zero failures

7. **ToolRegistry returns 4 schemas**
   - Given `ToolRegistry::with_defaults()`
   - When calling `.all_schemas()`
   - Then 4 JSON schema objects are returned, each with a `name` field

## Metadata
- **Complexity**: Medium
- **Labels**: tools, trait, async, tdd
- **Required Skills**: Rust, async/await, BoxFuture, serde_json
