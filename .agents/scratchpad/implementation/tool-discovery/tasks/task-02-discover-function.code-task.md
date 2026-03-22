---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: discover() Pure Function

## Description
Implement `pub fn discover(root: &Path) -> DiscoveryResult` in `ap/src/discovery/mod.rs`. This function reads `tools.toml` from the project root and `.ap/skills/*.toml` files (sorted alphabetically), parses them with serde, deduplicates tool names (first-wins), accumulates warnings for malformed files, and returns a `DiscoveryResult`. It never panics or returns `Result`.

## Background
This is the core I/O function for tool discovery. It must be infallible (never `panic!`, never `Result`) and pure in the sense that it has no side effects beyond reading the filesystem. All error conditions produce warnings in the result. The alphabetical sort of skill files is critical for determinism.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/tool-discovery/design.md` (Section 3.1, Section 5)

**Additional References:**
- `.agents/scratchpad/implementation/tool-discovery/context.md` (codebase patterns)
- `.agents/scratchpad/implementation/tool-discovery/plan.md` (Step 2 implementation notes)

**Note:** You MUST read the design document before beginning implementation. Pay special attention to the error handling table in Section 5.

## Technical Requirements
1. Function signature: `pub fn discover(root: &Path) -> DiscoveryResult`
2. Load order: `tools.toml` first (highest precedence), then `.ap/skills/*.toml` alphabetically
3. `tools.toml` missing → silent skip (no warning), `DiscoveryResult` with empty tools from this file
4. `tools.toml` malformed → push warning `"tools.toml: {parse_error}"`, skip file
5. `.ap/skills/` missing → silent skip (no warning)
6. `.ap/skills/x.toml` malformed → push warning `".ap/skills/x.toml: {parse_error}"`, skip file
7. Skill file `system_prompt` field → push to `system_prompt_additions`
8. Skill file tools → process same as `tools.toml` tools
9. Deduplication: maintain `HashSet<String>` of seen tool names; later duplicates get warning `"tool '{name}' in {file} conflicts with earlier definition — skipped"` and are skipped
10. `std::fs::read_dir` results MUST be sorted by `file_name()` before iteration
11. No `unwrap()` or `expect()` outside test modules
12. Use `toml::from_str::<ToolsFile>(&content)` and `toml::from_str::<SkillFile>(&content)` for parsing

## Dependencies
- Task 01: Discovery types + serde must be complete (types must exist)

## Implementation Approach
1. Write failing tests using `tempfile::TempDir` (RED): empty dir, valid tools.toml, malformed tools.toml, skills dir, duplicates, alphabetical order
2. Implement `discover()` body to make tests pass (GREEN)
3. Ensure no `unwrap()` in production code path (REFACTOR)
4. Run `cargo test --package ap discovery::` — all tests pass

## Acceptance Criteria

1. **Empty directory returns empty result**
   - Given an empty `TempDir`
   - When `discover(&tempdir.path())` is called
   - Then result has `tools: []`, `system_prompt_additions: []`, `warnings: []`

2. **Valid tools.toml parsed correctly**
   - Given a `TempDir` with a valid `tools.toml` containing 2 `[[tool]]` entries
   - When `discover()` is called
   - Then `result.tools` has 2 entries with correct fields and `result.warnings` is empty

3. **Malformed tools.toml produces warning without panic**
   - Given a `TempDir` with a `tools.toml` containing invalid TOML
   - When `discover()` is called
   - Then `result.tools` is empty, `result.warnings` has 1 entry containing "tools.toml"

4. **Malformed tool entry skips entire file**
   - Given a valid `tools.toml` with one good `[[tool]]` and one missing `command` field
   - When `discover()` is called
   - Then both tools are skipped (whole file) and `result.warnings` has 1 entry

5. **Skill file tools and system_prompt extracted**
   - Given `.ap/skills/ci.toml` with `system_prompt = "..."` and 1 `[[tool]]`
   - When `discover()` is called
   - Then `result.tools` has 1 tool, `result.system_prompt_additions` has 1 entry, no warnings

6. **Skill files processed alphabetically**
   - Given `.ap/skills/b.toml` (with "build" tool) and `.ap/skills/a.toml` (with "lint" tool)
   - When `discover()` is called
   - Then both tools are present; no warnings (no collision)

7. **Duplicate name: tools.toml wins over skill file**
   - Given `tools.toml` with tool "deploy" and `.ap/skills/ci.toml` with tool "deploy"
   - When `discover()` is called
   - Then `result.tools` has exactly 1 "deploy" tool (from tools.toml), `result.warnings` has 1 entry mentioning "deploy"

8. **Duplicate name: alphabetically first skill file wins**
   - Given `.ap/skills/a.toml` and `.ap/skills/b.toml` both defining tool "test"
   - When `discover()` is called
   - Then "test" from `a.toml` is kept, warning references `b.toml`

9. **system_prompt accumulates across multiple skill files**
   - Given `.ap/skills/a.toml` and `.ap/skills/b.toml` each with a `system_prompt`
   - When `discover()` is called
   - Then `result.system_prompt_additions` has 2 entries (in alphabetical file order)

10. **Param insertion order preserved**
    - Given a tool with params `c`, `a`, `b` in that TOML order
    - When `discover()` is called
    - Then `result.tools[0].params` keys are `["c", "a", "b"]` (IndexMap order)

11. **Unit Tests Pass**
    - Given the implementation is complete
    - When running `cargo test --package ap discovery::`
    - Then all 14 discovery unit tests pass

## Metadata
- **Complexity**: Medium
- **Labels**: discovery, io, rust, tempfile
- **Required Skills**: Rust, serde, TOML, tempfile
