---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Implement skill_injection_middleware

## Description
Add `skill_injection_middleware()` to `src/skills/mod.rs`. This function returns a closure compatible with `Middleware::pre_turn()`. The closure calls `loader.load()`, scores skills via `select_skills`, and — if any skills match — sets `conv.system_prompt` via `with_system_prompt()`. If no skills match, it returns `None` (no modification).

## Background
The middleware closure is the glue between the skill system and the `turn()` pipeline. It fires before every turn, re-loading skill files each time (so edits take effect immediately). The empty-guard is critical: returning `None` when no skills match prevents injecting a useless `"## Skills\n\n"` header into the system prompt.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/skill-system/design.md (Section 2 FR-7, Section 4.6/4.7, Appendix C.3)

**Additional References:**
- .agents/scratchpad/implementation/skill-system/context.md (Middleware::pre_turn signature, SkillsConfig shape)
- .agents/scratchpad/implementation/skill-system/plan.md (Step 5)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Function signature:
   ```rust
   pub fn skill_injection_middleware(
       loader: SkillLoader,
       config: SkillsConfig,
   ) -> impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static
   ```
2. Closure body:
   1. Call `loader.load()` → `Vec<Skill>`
   2. Call `select_skills(&skills, conv.messages(), config.max_injected)` → `Vec<&Skill>`
   3. **If empty: return `None`** (empty-guard — no injection)
   4. Call `skills_to_system_prompt(&selected)` → `String`
   5. Return `Some(conv.clone().with_system_prompt(prompt))`
3. `SkillsConfig` is referenced here but defined in Step 6 (`src/config.rs`). For now, define a minimal struct inline or use a stub — whichever compiles cleanly. The real struct is wired in Step 6.
4. `loader` and `config` must be `'static` — no borrowed references captured in the closure
5. No `unwrap()` or `expect()` in the closure body

## Dependencies
- Task 03 (Step 3): `Skill`, `SkillLoader` must exist
- Task 04 (Step 4): `select_skills`, `skills_to_system_prompt` must exist
- Task 01 (Step 1): `Conversation::with_system_prompt` must exist

## Implementation Approach
1. **RED**: Write 2 failing tests:
   - `middleware_empty_skills_returns_none`: SkillLoader with empty dirs + a conversation with unrelated message → closure returns `None`
   - `middleware_injects_system_prompt`: SkillLoader pointing at a tempdir with a matching skill → conversation with a matching message → closure returns `Some(conv)` with `system_prompt` set containing the skill name
2. **GREEN**: Implement `skill_injection_middleware`; stub `SkillsConfig` if needed
3. **REFACTOR**: Ensure `cargo test` (full suite) is green

## Acceptance Criteria

1. **Returns None when no skills match**
   - Given a `SkillLoader` with no matching skills and a `Conversation` with a message unrelated to any skill
   - When the returned closure is called with the conversation
   - Then it returns `None`

2. **Returns Some(conv) with system_prompt set when skills match**
   - Given a `SkillLoader` with a skill containing the word "docker" and a `Conversation` with message "how do I run docker"
   - When the returned closure is called
   - Then it returns `Some(conv)` where `conv.system_prompt` contains "docker"

3. **Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test -- middleware`
   - Then both tests pass

4. **Full suite still green**
   - Given the implementation is complete
   - When running `cargo test`
   - Then 0 failures

## Metadata
- **Complexity**: Medium
- **Labels**: skills, middleware, pre_turn, closure
- **Required Skills**: Rust, closures, lifetime management
