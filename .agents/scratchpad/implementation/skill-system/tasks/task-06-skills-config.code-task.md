---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: SkillsConfig in AppConfig

## Description
Add `SkillsConfig` to `src/config.rs` and integrate it into `AppConfig`. Extend `overlay_from_table()` to handle a `[skills]` section in the TOML config. If a stub `SkillsConfig` was created in Step 5, replace it with the real definition here.

## Background
`AppConfig` is the central configuration struct loaded from `~/.ap/config.toml`. The `overlay_from_table` function manually maps TOML table sections to config fields — new sub-structs require a new block there. The `SkillsConfig` needs `enabled`, `max_injected`, and optional `dirs` override fields.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/skill-system/design.md (Section 2 FR-8, Section 4.8, Appendix B.3)

**Additional References:**
- .agents/scratchpad/implementation/skill-system/context.md (overlay_from_table pattern, ProviderConfig as exemplar)
- .agents/scratchpad/implementation/skill-system/plan.md (Step 6)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Define `SkillsConfig` struct:
   ```rust
   #[derive(Debug, Clone)]
   pub struct SkillsConfig {
       pub enabled: bool,
       pub max_injected: usize,
       pub dirs: Option<Vec<PathBuf>>,  // explicit override; None = use defaults
   }
   ```
2. `impl Default for SkillsConfig`: `enabled=true`, `max_injected=5`, `dirs=None`
3. Add `pub skills: SkillsConfig` to `AppConfig` (with `Default` value)
4. Extend `overlay_from_table()` with a `[skills]` section handler:
   - If `enabled` key present: parse as bool, set it
   - If `max_injected` key present: parse as integer, set it
   - If `dirs` key present: parse as array of strings → `Vec<PathBuf>`, set `Some(dirs)`
5. Follow the exact same pattern as `[provider]` handling in `overlay_from_table`
6. If `SkillsConfig` was stubbed in Step 5 (`src/skills/mod.rs`), remove the stub and import from `src/config.rs`

## Dependencies
- Task 05 (Step 5): `SkillsConfig` may have been stubbed there — reconcile

## Implementation Approach
1. **RED**: Write 2 failing tests in `src/config.rs`:
   - `skills_config_default`: `SkillsConfig::default()` → `enabled=true`, `max_injected=5`, `dirs=None`
   - `skills_config_toml_overlay`: parse TOML with `[skills]\nmax_injected = 3\nenabled = false` → `max_injected=3`, `enabled=false`
2. **GREEN**: Implement `SkillsConfig`, `Default`, add to `AppConfig`, extend `overlay_from_table`
3. **REFACTOR**: Remove any stub from Step 5; `cargo clippy --all-targets -- -D warnings` clean

## Acceptance Criteria

1. **Default values correct**
   - Given `SkillsConfig::default()`
   - When fields are inspected
   - Then `enabled == true`, `max_injected == 5`, `dirs == None`

2. **TOML overlay updates fields**
   - Given a TOML string with `[skills]\nmax_injected = 3\nenabled = false`
   - When `AppConfig::from_toml_str` (or equivalent) is called
   - Then `config.skills.max_injected == 3` and `config.skills.enabled == false`

3. **Missing keys preserve defaults**
   - Given a TOML string with `[skills]` but no sub-keys
   - When parsed
   - Then all fields remain at their defaults

4. **Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test -- skills_config`
   - Then both tests pass

5. **Full suite still green**
   - Given the implementation is complete
   - When running `cargo test`
   - Then 0 failures

## Metadata
- **Complexity**: Low
- **Labels**: config, skills, toml, overlay
- **Required Skills**: Rust, TOML parsing, config patterns
