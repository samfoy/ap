---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: README.md

## Description
Write a complete, accurate `README.md` for the `ap` project. A new user should be able to follow it to install, configure, and run `ap` with no prior knowledge of the codebase.

## Background
Documentation is a first-class deliverable. The README must cover everything a user needs: installation, quick start, all configuration keys, all built-in tools, the hooks system with examples, session management, keybindings, and non-interactive mode. The extensions system (Rhai + dylib) was removed from the project per design amendment — do NOT document it. All documented behavior must match the actual implementation.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (Step 11)

**Note:** You MUST read the design document before beginning implementation. Appendix B (env vars) is useful for accurate hook documentation.

## Technical Requirements
1. `README.md` sections (in order):
   - **ap — AI Coding Agent** — one-line description
   - **Features** — bullet list of key features
   - **Installation** — `cargo install --path .` from source; prerequisites (Rust stable, AWS credentials)
   - **Quick Start** — `ap -p "your prompt"` and `ap` (TUI mode) examples
   - **AWS Setup** — credentials, region, model selection
   - **Configuration** — full `ap.toml` reference with every key documented and example values (`[provider]`, `[tools]`, `[hooks]` sections only — no `[extensions]`)
   - **Built-in Tools** — table: name, description, parameters for all 4 tools
   - **Hooks System** — lifecycle events table, env vars injected per hook type, shell script examples for each hook type
   - **Session Management** — `--session <id>`, auto-save behavior, file location
   - **Non-Interactive Mode** — `-p` flag, exit codes, use in scripts
   - **TUI Keybindings** — table: key, mode, action
   - **Contributing** — link to design document, test instructions
2. All hook env var names must match Appendix B of design.md exactly
3. All config keys must match `AppConfig` struct field names exactly (no `[extensions]` section — that was removed per design amendment)
4. Do NOT document the extensions system — it was removed from the codebase

## Dependencies
- All prior tasks (01-10 + extensions cleanup) must be complete — README documents the actual implementation

## Implementation Approach
1. Read `design.md` in full (especially Appendix B)
2. Run `ap --help` to verify actual CLI flags before documenting
3. Read `src/config.rs`, `src/hooks/runner.rs`, `src/tui/` to verify actual behavior
4. Write README section by section, cross-checking each section against the implementation
5. Review for accuracy: every config key, every env var, every tool parameter

## Acceptance Criteria

1. **Installation Instructions Work**
   - Given the README installation section
   - When a new user follows the instructions
   - Then they can build and run `ap` without referring to any other document

2. **Config Reference is Complete and Accurate**
   - Given the `[provider]`, `[tools]`, and `[hooks]` sections in README
   - When compared against `AppConfig` fields in `src/config.rs`
   - Then all config keys are documented with their defaults and valid values, and no removed sections (e.g. `[extensions]`) appear

3. **Hook Env Vars are Accurate**
   - Given the hooks system section in README
   - When compared against `src/hooks/runner.rs` implementation
   - Then all env var names (`AP_TOOL_NAME`, `AP_TOOL_PARAMS`, `AP_TOOL_RESULT`, etc.) match exactly

4. **No Extensions Documentation**
   - Given the README
   - When searched for "extension", "Rhai", "dylib", "libloading"
   - Then no such references appear (the extensions system was removed)

5. **README Reviewed for Accuracy**
   - Given the completed README
   - When a reviewer compares each documented behavior against the implementation
   - Then no behavioral claims are inaccurate or stale

## Metadata
- **Complexity**: Low
- **Labels**: documentation, readme
- **Required Skills**: Technical writing, Markdown
