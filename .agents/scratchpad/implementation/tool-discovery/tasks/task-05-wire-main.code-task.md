---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Wire Discovery into main.rs

## Description
Update `ap/src/main.rs` to call `discover()` at startup in both `run_headless` and `run_tui`, print warnings to stderr, register discovered `ShellTool`s with the `ToolRegistry` (before `Arc::new` wrap), and build + apply the system prompt from skill file additions.

## Background
This is the final integration step that makes all prior work user-visible. `main.rs` is the composition root — it calls `discover()`, surfaces warnings, and wires everything together. The critical ordering constraint is that `ShellTool`s must be registered BEFORE the registry is wrapped in `Arc::new` (since the Arc is immutable). The `run_tui` path already wraps tools in `Arc::new`; `run_headless` may not — read both paths carefully first.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/tool-discovery/design.md` (Section 3.7)

**Additional References:**
- `.agents/scratchpad/implementation/tool-discovery/context.md` (codebase patterns)
- `ap/src/main.rs` — both `run_headless` and `run_tui` functions
- `.agents/scratchpad/implementation/tool-discovery/plan.md` (Step 5)

**Note:** You MUST read `ap/src/main.rs` in full before making changes. Pay attention to where `ToolRegistry::with_defaults()` is called and where `Arc::new` wraps the registry in each function.

## Technical Requirements
1. In both `run_headless` and `run_tui`:
   a. Get project root: `let project_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));`
   b. Call `let discovery = discover(&project_root);`
   c. Print each warning: `for w in &discovery.warnings { eprintln!("ap: {w}"); }`
   d. Register ShellTools BEFORE `Arc::new`:
      ```rust
      let mut tools = ToolRegistry::with_defaults();
      for discovered in discovery.tools {
          tools.register(Box::new(ShellTool::new(discovered, project_root.clone())));
      }
      ```
   e. Build system prompt:
      ```rust
      let system_prompt: Option<String> = if discovery.system_prompt_additions.is_empty() {
          None
      } else {
          Some(discovery.system_prompt_additions.join("\n\n"))
      };
      ```
   f. Apply to conversation:
      ```rust
      let conv = match system_prompt {
          Some(sp) => conv.with_system_prompt(sp),
          None => conv,
      };
      ```
2. Add imports: `use crate::discovery::discover;` and `use crate::tools::ShellTool;`
3. No `unwrap()` allowed (use `unwrap_or_else` for `current_dir`)
4. `cargo build --package ap` must pass
5. `cargo test --package ap` must pass (all existing tests + new tests from prior steps)
6. `cargo clippy --package ap -- -D warnings` must pass

## Dependencies
- Task 02: `discover()` function must exist
- Task 03: `ShellTool` must exist
- Task 04: `Conversation::with_system_prompt()` must exist

## Implementation Approach
1. Read `ap/src/main.rs` fully to understand current structure (no TDD needed here — this is wiring)
2. Add imports for `discover` and `ShellTool`
3. Apply changes to `run_headless` first (simpler path)
4. Apply same changes to `run_tui` (check Arc ordering carefully)
5. Run `cargo build --package ap` — fix any errors
6. Run `cargo test --package ap` — all tests pass
7. Run `cargo clippy --package ap -- -D warnings` — clean

## Acceptance Criteria

1. **Warnings printed to stderr on startup**
   - Given a project with a malformed `.ap/skills/bad.toml`
   - When `ap` starts
   - Then `ap: .ap/skills/bad.toml: ...` appears on stderr and `ap` does not crash

2. **Discovered tools available in Claude's context**
   - Given a `tools.toml` with a tool named "greet"
   - When `ap` starts
   - Then `ToolRegistry` contains "greet" in addition to the built-in tools

3. **System prompt applied from skill files**
   - Given `.ap/skills/dev.toml` with `system_prompt = "Use the greet tool"`
   - When `ap` starts
   - Then `Conversation.system_prompt` is `Some("Use the greet tool")`

4. **No warnings for clean project**
   - Given a project with no `tools.toml` and no `.ap/skills/` directory
   - When `ap` starts
   - Then no lines beginning with `"ap: "` appear on stderr

5. **Duplicate tool warning surfaced**
   - Given `tools.toml` and `.ap/skills/ci.toml` both defining a tool named "deploy"
   - When `ap` starts
   - Then stderr contains `"ap: tool 'deploy' in .ap/skills/ci.toml conflicts with earlier definition — skipped"`

6. **Build and test gates pass**
   - Given all prior tasks complete
   - When running `cargo build --package ap && cargo test --package ap && cargo clippy --package ap -- -D warnings`
   - Then all three commands exit 0

## Metadata
- **Complexity**: Low
- **Labels**: main, wiring, integration, rust
- **Required Skills**: Rust, async, composition
