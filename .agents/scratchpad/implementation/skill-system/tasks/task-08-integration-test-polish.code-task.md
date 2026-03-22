---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Integration test + clippy/doc polish

## Description
Write `tests/skill_injection.rs` — a full integration test covering the `SkillLoader → select_skills → skill_injection_middleware` pipeline end-to-end without any LLM call. Then run `cargo clippy --all-targets -- -D warnings` and add missing `///` doc comments on all public API items in `src/skills/mod.rs`.

## Background
This is the final polish step. The integration test exercises the complete skill pipeline: multi-directory later-wins override, TF-IDF selection, and middleware injection. It uses `tempfile::TempDir` (already in Cargo.toml) so it works without any filesystem setup. The test also covers the adversarial path: middleware with no matching messages returns `None`.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/skill-system/design.md (Section 5, Appendix A)

**Additional References:**
- .agents/scratchpad/implementation/skill-system/plan.md (Step 8, Integration Test scenario)
- .agents/scratchpad/implementation/skill-system/context.md (tempfile usage, test module allow attrs)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Create `tests/skill_injection.rs` with a single integration test `skill_pipeline_end_to_end`:
   - Set up two temp dirs: `global/` and `project/`
   - Write `global/git.md` with body "Use git to commit and push changes"
   - Write `global/shared.md` with body "GLOBAL version"
   - Write `project/docker.md` with body "Use docker to build and run containers"
   - Write `project/shared.md` with body "PROJECT version"
   - Assert `SkillLoader::new(vec![global, project]).load()` returns 3 skills
   - Assert `shared.body == "PROJECT version"` (later-wins)
   - Build a `Conversation` with message "I need help with git commit"
   - Assert `select_skills` returns git skill; docker skill is NOT in results
   - Call `skill_injection_middleware(loader, config)(conv)` → assert `Some`, `system_prompt` contains "git"
   - Build empty `Conversation` (no messages) → assert middleware returns `None`
2. Run `cargo clippy --all-targets -- -D warnings` → fix all warnings
3. Add `///` doc comments to all `pub` items in `src/skills/mod.rs` that lack them
4. Ensure `cargo test --all-targets` exits 0

## Dependencies
- All previous tasks (1-7) must be complete

## Implementation Approach
1. **RED**: Write `tests/skill_injection.rs` — the file itself won't compile until all prior steps are complete, so this step confirms full integration
2. **GREEN**: Tests pass with no modifications needed (prior steps already correct) — if any test fails, debug the root cause in the relevant module
3. **POLISH**:
   - Run `cargo clippy --all-targets -- -D warnings`; fix any warnings
   - Add doc comments to public items in `src/skills/mod.rs`
4. **FINAL**: `cargo test --all-targets` green; `cargo build --release` succeeds

## Acceptance Criteria

1. **Later-wins override confirmed**
   - Given `SkillLoader` with global and project dirs both having `shared.md`
   - When `load()` is called
   - Then exactly 3 skills returned; `shared.body == "PROJECT version"`

2. **TF-IDF selects relevant skill**
   - Given the test setup above and message "I need help with git commit"
   - When `select_skills` is called
   - Then git skill included, docker skill excluded

3. **Middleware injects system_prompt**
   - Given the full pipeline with a matching message
   - When `skill_injection_middleware(loader, config)(conv)` is called
   - Then returns `Some(conv)` with `system_prompt` containing skill content about git

4. **Middleware returns None for empty conversation**
   - Given a `Conversation` with no messages
   - When the middleware closure is called
   - Then returns `None`

5. **Clippy clean**
   - Given the complete implementation
   - When running `cargo clippy --all-targets -- -D warnings`
   - Then exits 0 with no warnings

6. **Full test suite passes**
   - Given the complete implementation
   - When running `cargo test --all-targets`
   - Then 0 failures

7. **Release build succeeds**
   - Given the complete implementation
   - When running `cargo build --release`
   - Then exits 0

## Metadata
- **Complexity**: Low
- **Labels**: testing, integration, clippy, docs, polish
- **Required Skills**: Rust, integration testing, tempfile
