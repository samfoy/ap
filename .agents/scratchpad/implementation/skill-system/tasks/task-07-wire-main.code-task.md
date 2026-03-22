---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Wire skills into main.rs

## Description
Update `src/main.rs` to wire the skill injection middleware into both `run_headless` and `run_tui`. Build a `SkillLoader` from resolved directories (combining global `~/.ap/skills/` + project `./.ap/skills/` with any `config.skills.dirs` override), then conditionally add the middleware via `.pre_turn()` when `config.skills.enabled`.

## Background
`main.rs` is the composition root. Both execution paths (`run_headless`, `run_tui`) build a `Middleware` chain using the builder pattern. The skill closure is added via `.pre_turn(skill_injection_middleware(loader, config.skills.clone()))`. No new extension points are needed — the existing `Middleware::pre_turn` chain handles this transparently.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/skill-system/design.md (Section 4.9, FR-9)

**Additional References:**
- .agents/scratchpad/implementation/skill-system/context.md (main.rs Middleware builder pattern, dirs crate usage)
- .agents/scratchpad/implementation/skill-system/plan.md (Step 7)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Determine skill directories:
   - If `config.skills.dirs` is `Some(dirs)`: use those dirs directly
   - Otherwise: build default list from `dirs::home_dir().unwrap_or_default().join(".ap/skills/")` (global) + `PathBuf::from(".ap/skills/")` (project), filtering to dirs that exist
2. Build `SkillLoader::new(skill_dirs)` (even if dirs list is empty — `load()` returns empty vec gracefully)
3. If `config.skills.enabled`: add `.pre_turn(skill_injection_middleware(loader, config.skills.clone()))` to the middleware chain in BOTH `run_headless` and `run_tui`
4. No `unwrap()` or `expect()` — `dirs::home_dir()` returns `Option<PathBuf>`, use `unwrap_or_default()`
5. No new unit tests required — verified by `cargo test` passing + `cargo build` succeeding

## Dependencies
- Task 03 (Step 3): `SkillLoader` must exist
- Task 05 (Step 5): `skill_injection_middleware` must exist
- Task 06 (Step 6): `SkillsConfig` must be in `AppConfig`

## Implementation Approach
1. **Read** `src/main.rs` completely before editing — understand existing middleware chain
2. **Update** `run_headless`: add skill dir resolution + conditional middleware wiring
3. **Update** `run_tui`: same pattern
4. **Verify**: `cargo build` succeeds, `cargo test` full suite green
5. **Manual smoke test**: create a skill file in `~/.ap/skills/` and run `ap` with a related query to confirm injection (verify via verbose output or debug logging)

## Acceptance Criteria

1. **Skills enabled by default wires middleware**
   - Given `config.skills.enabled == true` (default)
   - When `run_headless` or `run_tui` builds the middleware chain
   - Then the skill injection closure is present in the pre_turn chain

2. **Skills disabled skips middleware**
   - Given `config.skills.enabled == false`
   - When `run_headless` or `run_tui` builds the middleware chain
   - Then no skill injection closure is added

3. **Default dirs are resolved correctly**
   - Given no `config.skills.dirs` override
   - When dirs are resolved
   - Then `~/.ap/skills/` and `./.ap/skills/` are in the list (if they exist)

4. **Build succeeds**
   - Given the implementation is complete
   - When running `cargo build`
   - Then exits 0

5. **Full suite still green**
   - Given the implementation is complete
   - When running `cargo test`
   - Then 0 failures

## Metadata
- **Complexity**: Low
- **Labels**: main, wiring, middleware, composition-root
- **Required Skills**: Rust, main.rs composition patterns
