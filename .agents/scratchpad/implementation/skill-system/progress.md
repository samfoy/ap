# Implementation Progress: Skill System

## Current Step
**Step 8** — Integration test + clippy/doc polish

## Active Wave

| Step | Code Task File | Runtime Task ID | Status |
|------|---------------|-----------------|--------|
| 8 | tasks/task-08-integration-test-polish.code-task.md | task-1774199280-0809 | open |

## Completed Waves

| Step | Description | Completed |
|------|-------------|-----------|
| — | Task writing (code task files) | 2026-03-22 |
| 1 | Conversation.system_prompt field + builder | 2026-03-22 |
| 2 | Thread system_prompt through Provider trait + BedrockProvider | 2026-03-22 |
| 3 | Skill struct, SkillLoader::new/load, frontmatter parsing | 2026-03-22 |
| 4 | select_skills TF-IDF scoring + skills_to_system_prompt formatter | 2026-03-22 |
| 5 | skill_injection_middleware pre_turn closure | 2026-03-22 |
| 6 | SkillsConfig in AppConfig with TOML overlay | 2026-03-22 |

| 7 | Wire skills into main.rs | 2026-03-22 |

## Step 4: select_skills TF-IDF + skills_to_system_prompt (task-1774199280-b091)

### RED
- Wrote 4 failing tests: `select_skills_returns_top_n`, `select_skills_excludes_zero_score`, `select_skills_empty_messages`, `skills_to_system_prompt_format`
- Tests failed correctly

### GREEN
- Implemented `tokenize()` helper, `select_skills()`, `skills_to_system_prompt()`
- IDF formula: `ln(N/df + 1)` — corrected from initial `ln(N/(df+1))` which gave 0 for unique terms in small corpora
- Fixed test `select_skills_returns_top_n` to use skill bodies that produce deterministic ordering under this formula

### REFACTOR
- Fixed 2 clippy lints: `redundant_closure_for_method_calls` (`.map(str::to_lowercase)`) and `redundant_closure` (`.flat_map(tokenize)`)
- All 111 tests pass, clippy clean

