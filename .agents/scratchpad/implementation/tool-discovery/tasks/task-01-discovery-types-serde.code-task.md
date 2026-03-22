---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Discovery Types + TOML Serde

## Description
Create `ap/src/discovery/mod.rs` with the core data types for tool discovery: `DiscoveryResult`, `DiscoveredTool`, `ParamSpec`, and private intermediate serde structs (`ToolsFile`, `SkillFile`, `RawTool`). Add `indexmap` to `Cargo.toml` and register the new module in `lib.rs`. No I/O in this step — pure types with serde derive.

## Background
Tool discovery needs typed Rust structs that deserialize from project TOML files. This step establishes the data layer that all subsequent steps depend on. The `indexmap` crate is required for `IndexMap<String, ParamSpec>` to preserve param insertion order (matching author intent). All serde attributes must be correct now — later steps won't revisit these types.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/tool-discovery/design.md` (Section 3.1)

**Additional References:**
- `.agents/scratchpad/implementation/tool-discovery/context.md` (codebase patterns)
- `.agents/scratchpad/implementation/tool-discovery/plan.md` (overall strategy)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Add `indexmap = { version = "2", features = ["serde"] }` to `ap/Cargo.toml` dependencies
2. Add `pub mod discovery;` to `ap/src/lib.rs`
3. Create `ap/src/discovery/mod.rs` with:
   - `pub struct DiscoveryResult { pub tools: Vec<DiscoveredTool>, pub system_prompt_additions: Vec<String>, pub warnings: Vec<String> }`
   - `pub struct DiscoveredTool { pub name: String, pub description: String, pub params: IndexMap<String, ParamSpec>, pub command: String }`
   - `pub struct ParamSpec { pub description: String, pub required: bool }` with `#[serde(default = "default_required")]` on `required` and `fn default_required() -> bool { true }`
   - Private `ToolsFile`: `#[serde(rename = "tool", default)] tools: Vec<RawTool>`
   - Private `SkillFile`: `system_prompt: Option<String>` + `#[serde(rename = "tool", default)] tools: Vec<RawTool>`
   - Private `RawTool`: `name`, `description`, `command`, `#[serde(default)] params: IndexMap<String, ParamSpec>`
4. All three public structs derive `Debug`, `Clone`
5. `ToolsFile`, `SkillFile`, `RawTool`, `ParamSpec` derive `Deserialize`; `DiscoveredTool` and `ParamSpec` also derive `Serialize` (for future use)
6. No `unwrap()` or `expect()` outside test modules (clippy lint compliance)

## Dependencies
- None (this is Step 1 — the foundation)

## Implementation Approach
1. Write failing serde tests first (RED)
2. Add `indexmap` to `Cargo.toml`
3. Add `pub mod discovery;` to `lib.rs`
4. Implement structs with correct serde attributes to make tests pass (GREEN)
5. Run `cargo check --package ap` and fix any type errors (REFACTOR)

## Acceptance Criteria

1. **Valid tools.toml parses correctly**
   - Given a TOML string with a `[[tool]]` section containing `name`, `description`, `command`, and `[tool.params.foo]`
   - When deserialized as `ToolsFile`
   - Then `tools[0].name`, `.description`, `.command`, `.params["foo"]` all match the TOML values

2. **Valid skill file parses correctly**
   - Given a TOML string with `system_prompt = "..."` and a `[[tool]]` section
   - When deserialized as `SkillFile`
   - Then `system_prompt == Some("...")` and `tools` is non-empty

3. **ParamSpec required defaults to true**
   - Given a `[tool.params.foo]` table with only `description = "..."` (no `required` field)
   - When deserialized as `ParamSpec`
   - Then `required == true`

4. **ParamSpec required false is explicit**
   - Given a `[tool.params.foo]` table with `required = false`
   - When deserialized as `ParamSpec`
   - Then `required == false`

5. **Empty tools.toml does not error**
   - Given a TOML string with no `[[tool]]` sections
   - When deserialized as `ToolsFile`
   - Then `tools` is an empty `Vec` (not an error)

6. **Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test --package ap discovery::`
   - Then all 5 serde unit tests pass and `cargo check` is clean

## Metadata
- **Complexity**: Low
- **Labels**: discovery, types, serde, rust
- **Required Skills**: Rust, serde, TOML
