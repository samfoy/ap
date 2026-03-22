---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Extensions System (Rhai + Dylib)

## Description
Implement `src/extensions/` with a `Registry`, `ExtensionLoader`, Rhai script loading (`RhaiTool`), and Rust dylib loading. Extensions are discovered from `~/.ap/extensions/` and `./.ap/extensions/`. Both Rhai (`.rhai`) and dylib (`.dylib`/`.so`) are first-class in v1. Hook/panel/message-interceptor surfaces are collected but not invoked in v1 (stubs).

## Background
Extensions let users add custom tools to `ap` without recompiling. Rhai scripts are the safe, sandboxed path. Rust dylibs are the power-user path with a clear UB warning. The critical implementation pitfalls are:
1. `rhai::Engine` requires `features = ["sync"]` to be Send+Sync (already in Cargo.toml from step 1)
2. `Path::extension()` returns `Option<&OsStr>` — must use `.and_then(|e| e.to_str())` for string matching
3. `libloading::Library` must be held in `ExtensionLoader.libraries: Vec<Library>` — dropping it calls `dlclose()` and causes UAF

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (Step 6 and test table)

**Note:** You MUST read the design document before beginning implementation. Section 4.8 covers the full extensions design including the 3 FAILs that were fixed in the final design revision.

## Technical Requirements
1. `src/extensions/mod.rs`:
   - `Extension` trait: `name(&self) -> &str`, `version(&self) -> &str`, `register(&self, registry: &mut Registry)`
   - `Panel` trait: stub — `name(&self) -> &str` (collected but not rendered v1)
   - `MessageInterceptor` trait: stub — `intercept(&self, msg: &Message) -> Message` (collected but not invoked v1)
   - `Registry` struct:
     - `tools: Vec<Box<dyn Tool>>`
     - `hooks: Vec<String>` (stub — hook paths from extensions, not invoked v1)
     - `panels: Vec<Box<dyn Panel>>` (stub)
     - `message_interceptors: Vec<Box<dyn MessageInterceptor>>` (stub)
     - `register_tool(&mut self, tool: Box<dyn Tool>)`
     - `register_hook(&mut self, path: String)`
     - `register_panel(&mut self, panel: Box<dyn Panel>)`
     - `register_message_interceptor(&mut self, interceptor: Box<dyn MessageInterceptor>)`
2. `src/extensions/rhai_loader.rs` — `RhaiTool`:
   - Wraps a Rhai script file as `Box<dyn Tool>`
   - Rhai script must define: `fn name() -> String`, `fn description() -> String`, `fn schema() -> Map`, `fn execute(params: Map) -> Map` (returns `{content: String, is_error: bool}`)
   - `RhaiTool::load(path: &Path) -> anyhow::Result<RhaiTool>` — reads script, calls `name()` and `description()` at load time to validate; returns error on syntax error or missing functions
   - Rhai engine: use `Engine::new()` with file I/O and system modules disabled (sandbox)
   - `rhai = { version = "1", features = ["sync"] }` — required for Send+Sync
3. `src/extensions/dylib_loader.rs`:
   - `ExtensionLoader { libraries: Vec<Library>, warnings: Vec<String> }`
   - `discover_and_load(&mut self, registry: &mut Registry)`:
     - Scan `~/.ap/extensions/` and `./.ap/extensions/` (skip if dir doesn't exist)
     - For each entry: `match entry.path().extension().and_then(|e| e.to_str()) { Some("rhai") => load_rhai(...), Some("dylib") | Some("so") => load_dylib(...), _ => {} }`
   - `load_rhai(path, registry, warnings)` — calls `RhaiTool::load`, on success `registry.register_tool(Box::new(rhai_tool))`, on error push to warnings
   - `load_dylib(path, registry, warnings) -> anyhow::Result<Library>`:
     - `unsafe { Library::new(path) }` — on error: push warning, return Err
     - `unsafe { lib.get::<extern "C" fn(*mut Registry)>(b"ap_extension_init\0") }` — on error: push warning, return Err
     - Call the function pointer with `registry`
     - Return `Ok(lib)` — caller pushes to `self.libraries`
   - Explicit doc comment: "Dropping Library calls dlclose(); always store returned Library in self.libraries"
   - Prominent safety warning: dylib extensions are unsafe by design; toolchain/crate version mismatch causes UB; prefer Rhai

## Dependencies
- Task 01 (project scaffold) — `rhai`, `libloading`, `dirs` declared
- Task 03 (tool trait) — `Tool`, `ToolResult` types
- Task 04 (provider) — `Message` type for `MessageInterceptor`

## Implementation Approach
1. Write all 5 unit tests first (RED):
   - `test_load_valid_rhai_tool` — create temp `.rhai` file with all 4 fns; verify loads OK
   - `test_rhai_execute_returns_result` — load tool, call execute, verify ToolResult
   - `test_rhai_syntax_error_returns_warning` — broken script → `RhaiTool::load` returns Err
   - `test_rhai_missing_function_returns_warning` — script missing `execute` fn → Err
   - `test_dylib_missing_symbol_returns_warning` — on a non-dylib file → warning collected, no panic
2. Implement `Registry` struct
3. Implement `RhaiTool` with sandboxed engine
4. Implement `ExtensionLoader` with correct `OsStr` matching and `Library` storage
5. All 5 tests pass

## Acceptance Criteria

1. **Valid Rhai Tool Loads**
   - Given a `.rhai` file defining `name()`, `description()`, `schema()`, `execute(params)`
   - When `RhaiTool::load(path)` is called
   - Then returns `Ok(RhaiTool)` with correct name and description

2. **Rhai Tool Executes**
   - Given a loaded `RhaiTool` whose `execute` returns `#{content: "42", is_error: false}`
   - When `tool.execute(params).await` is called
   - Then returns `ToolResult { content: "42", is_error: false }`

3. **Rhai Syntax Error Returns Err**
   - Given a `.rhai` file with invalid syntax `fn name() { %%%`
   - When `RhaiTool::load(path)` is called
   - Then returns `Err(...)` — no panic

4. **Rhai Missing Function Returns Err**
   - Given a `.rhai` file that defines `name()` but not `execute(params)`
   - When `RhaiTool::load(path)` is called
   - Then returns `Err(...)` with message indicating missing function

5. **Dylib Missing Symbol → Warning, No Panic**
   - Given a path to a file that is not a valid dylib (or does not export `ap_extension_init`)
   - When `load_dylib(path, registry, &mut warnings)` is called
   - Then a warning is pushed to `warnings` and no panic occurs

6. **All 5 Extension Tests Pass**
   - Given the implementation is complete
   - When running `cargo test extensions`
   - Then all 5 extension tests pass

7. **Extension Discovery Doesn't Crash on Missing Dir**
   - Given `~/.ap/extensions/` does not exist
   - When `discover_and_load` is called
   - Then returns without error (missing directories are silently skipped)

## Metadata
- **Complexity**: High
- **Labels**: extensions, rhai, dylib, libloading, unsafe
- **Required Skills**: Rust, Rhai scripting, libloading, FFI, OsStr
