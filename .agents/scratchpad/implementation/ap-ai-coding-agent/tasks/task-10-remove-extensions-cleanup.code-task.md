---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: Remove Extensions System

## Description
Delete the `src/extensions/` module entirely and remove the `rhai` and `libloading` crate
dependencies from `Cargo.toml`. Update `main.rs`, `app.rs`, and `ap.toml.example` to remove
all references to the extensions system. The hooks system is retained — only the plugin/scripting
machinery is removed.

## Background
Design amendment (2026-03-22): Extensions (Rhai scripting + Rust dylib) are a design mistake.
With AI agents, anyone who wants custom tools just has the agent edit the source code. Plugin
systems add complexity without value in the agent era. Hooks stay — shell lifecycle hooks are
genuinely useful. Everything under `src/extensions/` must be deleted and all references cleaned up.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md (section 4.8 — to understand what must be removed)

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (overall strategy)

**Note:** Read the source files first to understand what references exist before deleting.

## Technical Requirements
1. Delete `ap/src/extensions/` directory and all files within it (`mod.rs`, `rhai_loader.rs`, `dylib_loader.rs`)
2. Remove from `ap/Cargo.toml`:
   - `rhai = { version = "1", features = ["sync"] }` (or however it appears)
   - `libloading = "..."` (or however it appears)
3. Remove from `ap/src/main.rs`:
   - `mod extensions;` declaration
   - Any `use crate::extensions::...` imports
   - Any `ExtensionLoader` instantiation / `discover_and_load` calls
4. Remove from `ap/src/app.rs` (if present):
   - Any `extensions` field or usage
5. Remove `[extensions]` section from `ap/ap.toml.example` (if present)
6. Update `ap/ap.toml.example` to remove the `[extensions]` section and any extension-related comments
7. `cargo build --release` must succeed with zero warnings after cleanup

## Dependencies
- Task 09 (TUI) — must be complete before this cleanup

## Implementation Approach
1. Identify all files that reference extensions: `grep -r "extensions\|rhai\|libloading\|ExtensionLoader" ap/src/`
2. Remove `src/extensions/` directory
3. Remove deps from Cargo.toml
4. Clean up main.rs / app.rs references
5. Clean up ap.toml.example
6. `cargo build --release` → zero warnings
7. `cargo test` → all tests pass (extension tests will be removed along with the module)

## Acceptance Criteria

1. **Extensions Module Deleted**
   - Given the repository after cleanup
   - When `ls ap/src/extensions/` is run
   - Then the directory does not exist (command fails with "No such file or directory")

2. **Cargo.toml Has No Extension Deps**
   - Given `ap/Cargo.toml` after cleanup
   - When `grep -E "rhai|libloading" ap/Cargo.toml` is run
   - Then the command returns empty (no matches)

3. **Build Succeeds With Zero Warnings**
   - Given the cleaned-up codebase
   - When `cargo build --release` is run in the `ap/` directory
   - Then exit code is 0 and stderr contains no warnings

4. **All Tests Pass**
   - Given the cleaned-up codebase
   - When `cargo test` is run
   - Then all tests pass (extension tests are removed; all remaining tests continue to pass)

5. **No Dangling References**
   - Given the cleaned-up codebase
   - When `grep -r "extensions\|rhai\|libloading\|ExtensionLoader\|RhaiTool\|dylib_loader\|rhai_loader" ap/src/` is run
   - Then no matches are found in any `.rs` file

## Metadata
- **Complexity**: Low
- **Labels**: cleanup, refactor, extensions, design-amendment
- **Required Skills**: Rust, Cargo
