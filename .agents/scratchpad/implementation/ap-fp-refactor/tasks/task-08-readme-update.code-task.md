---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: README Update — FP Architecture Docs

## Description
Update `ap/README.md` to document the new functional pipeline architecture introduced by the FP refactor. Remove or replace references to the old `AgentLoop`/`UiEvent` model. Add a section on the `Middleware` chain API and how developers can extend `ap` by inserting closures into the pipeline.

## Background
The refactor replaced `AgentLoop` with a pure `turn()` function, introduced a `Middleware` chain for composable behavior, and made `Conversation` an immutable value type. `main.rs` now reads as a recipe. The README still describes the old architecture and doesn't mention the middleware API.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-fp-refactor/design.md (if present)
- Current README: ap/README.md

**Additional References:**
- ap/src/types.rs — Conversation, TurnEvent, Middleware types
- ap/src/turn.rs — pure turn() function signature
- ap/src/middleware.rs — Middleware builder + shell_hook_bridge
- ap/src/main.rs — recipe-style startup for reference

## Technical Requirements
1. README must accurately describe the `turn()` function as the core pipeline (pure, returns `(Conversation, Vec<TurnEvent>)`)
2. README must document the `Middleware` chain API with a concrete example (e.g., logging every bash call)
3. README must explain how to add a custom pre-tool middleware closure in `main.rs`
4. Shell hooks section must note that shell hooks are wrapped as middleware at startup (bridge adapter)
5. Session persistence section must mention `save_conversation`/`load_conversation` and `Conversation` serialization
6. Remove or update any references to `AgentLoop` or `UiEvent`
7. `cargo build --release` must still pass (README changes are doc-only, no code changes)

## Dependencies
- Steps 01–07 completed (all structural changes in place)

## Implementation Approach
1. Read the current README fully
2. Read `src/types.rs`, `src/turn.rs`, `src/middleware.rs`, `src/main.rs` for accurate documentation
3. Update the Architecture / Extending section with the new pipeline model
4. Add a "Middleware API" section with example closures
5. Update Shell Hooks section to describe the bridge adapter
6. Update Session Persistence to reflect `Conversation` serialization
7. Remove all `AgentLoop`/`UiEvent` references

## Acceptance Criteria

1. **Architecture Section Accurate**
   - Given the README is read
   - When the "Architecture" section is found
   - Then it describes `turn()` as a pure pipeline returning `(Conversation, Vec<TurnEvent>)` with no mention of `AgentLoop`

2. **Middleware API Documented**
   - Given the README is read
   - When the "Extending" or "Middleware" section is found
   - Then it includes a code example showing how to push a closure into `middleware.pre_tool`

3. **Shell Hooks Bridge Explained**
   - Given the README is read
   - When the "Shell Hooks" section is found
   - Then it explains that shell hook scripts are wrapped as `Middleware` closures at startup via `shell_hook_bridge`

4. **No Stale References**
   - Given the README is read
   - When grepping for `AgentLoop` or `UiEvent`
   - Then zero matches are found

5. **Build Still Passes**
   - Given the README is updated
   - When running `cargo build --release`
   - Then it exits 0 with zero warnings

## Metadata
- **Complexity**: Low
- **Labels**: docs, readme
- **Required Skills**: Technical writing, Rust
