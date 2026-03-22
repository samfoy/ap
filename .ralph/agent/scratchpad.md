# Scratchpad

## 2026-03-22 — Initial orientation

### Objective
Implement offline, pure-Rust skill injection using TF-IDF relevance scoring in the `ap` project (worktree at `/Users/sam.painter/Projects/ap-worktrees/skill-system/ap`).

### 8 ordered steps from PROMPT.md
1. Extend `Conversation` with `system_prompt: Option<String>` + builder
2. Thread `system_prompt` through `Provider` trait and `BedrockProvider`
3. `src/skills/mod.rs` — `Skill`, `SkillLoader` with frontmatter parsing
4. `select_skills()` TF-IDF scoring + `skills_to_system_prompt()` formatter
5. `skill_injection_middleware()` — pre_turn closure
6. `SkillsConfig` in `AppConfig` (max_injected, dir overrides)
7. Wire into `run_headless` and `run_tui` in `main.rs`
8. Integration test + clippy/doc polish

### Key files
- `ap/src/types.rs` — Conversation, Message types
- `ap/src/provider/mod.rs` — Provider trait
- `ap/src/provider/bedrock.rs` — BedrockProvider
- `ap/src/turn.rs` — pure turn() function
- `ap/src/middleware.rs` — Middleware chain
- `ap/src/config.rs` — AppConfig
- `ap/src/lib.rs` — module registration
- `ap/src/main.rs` — wiring

### Plan
Create 8 tasks corresponding to each step, start with step 1 immediately in this iteration (it's small and safe).
The tasks must be done in order due to compile-time dependencies.

## 2026-03-22 — Inquisitor pass

PROMPT.md is very detailed. Key requirement gap identified:

`system_prompt` on `Conversation` uses `#[serde(default)]` meaning it persists in sessions.
The middleware is spec'd to *append* to existing `system_prompt`. This creates accumulation
across sessions — each load + turn would pile more skill content on top of stale injected
content from prior sessions.

Two clean options:
1. `#[serde(skip)]` — transient field, middleware always replaces (simplest)
2. Keep serialisation, middleware always replaces (not appends)
3. Keep serialisation + append (spec'd, but accumulation bug)

Asked as Q1 in idea-honing.md. Waiting for answer before emitting requirements.complete.

## 2026-03-22 — Design Review

### Review Result: REJECTED (2 FAILs)

**FAIL-1: `skill_injection_middleware` return type/wiring is internally contradictory**
- FR-7 (requirements.md): `-> Middleware`
- Design Section 4.8: shows `.remove(0)` approach, then says "Actually..." and abandons it
- Design Appendix C.3: "wires as a single `pre_turn` closure, not as a full `Middleware` struct"
- Design D-01: concludes with `-> Middleware` + `middleware.pre_turn.extend(skill_mw.pre_turn)`

Three different wiring patterns described with no final canonical answer. An implementer cannot resolve this without guessing.

**FAIL-2: Middleware behavior when `select_skills` returns empty is contradictory**
- FR-7 says unconditionally: "3. Calls `skills_to_system_prompt()` on the result" → "4. Sets `conv.system_prompt = Some(block)`"
- Section 6 (error table) says: "No skills match query → Middleware sets `system_prompt = None` (or omits the call entirely)"
- These contradict. If empty, calling `skills_to_system_prompt([])` produces a `"## Skills\n\n"` header with no content — useless noise injected into every turn where skills don't match. The conditional guard before calling `skills_to_system_prompt` needs to be explicit in FR-7.

## 2026-03-22 — Design drafted

Design document written to `.agents/scratchpad/implementation/skill-system/design.md`.

Key design decisions captured:
- `system_prompt` is `#[serde(skip)]` transient (from Q1 answer)
- Provider trait change (step 2) is most invasive — done early to minimize breakage accumulation
- `SkillLoader::load()` called every turn (intentional — no caching; files may change)
- TF-IDF is pure Rust: tf * idf, lowercase+alphanumeric tokenization, score 0 = excluded
- `skill_injection_middleware` returns `Middleware` (spec-faithful); wiring in main.rs merges pre_turn vecs
- Frontmatter parsed with simple line scanning (no YAML crate)
- 8 implementation tasks already exist as open tasks, ordered by dependency

Emitting design.drafted → Design Critic next.

## 2026-03-22 — Q2 resolved, requirements finalised

A2 received: `skill_injection_middleware` returns a closure, caller chains via `.pre_turn(...)`.
No `Middleware::merge` needed.

FR-7 updated: closure signature + explicit empty-guard (return `None` if no skills match).
FR-9 updated: wiring uses `.pre_turn(skill_injection_middleware(...))` chained on `shell_hook_bridge(...)`.
Resolved decisions table extended with Q2 and FAIL-2 entries.

Requirements now complete — no outstanding gaps. Emitting `requirements.complete` for Architect redesign.

## 2026-03-22 — Design rewritten (post-requirements.complete)

Design.md fully rewritten to be consistent with finalized requirements:
- FR-7: `skill_injection_middleware` returns `impl Fn(...)` closure, NOT `Middleware` struct
- FR-7: explicit empty-guard — returns `None` if `select_skills` returns empty
- FR-9: wiring uses `.pre_turn(skill_injection_middleware(...))` — no `Middleware::merge`
- Removed all contradictory wiring patterns from Section 4.8 and Appendix D-01
- Decision table in Appendix D is clean with all 5 resolved decisions

All contradictions from design.rejected (FAIL-1, FAIL-2) are resolved.
Emitting design.drafted → Design Critic.

## 2026-03-22 — Design Review: APPROVED

Reviewed rewritten design.md against requirements.md and codebase.

### Scorecard: All PASS
- Completeness: FR-1..FR-9 all addressed, error handling table in Section 6, edge cases covered
- Feasibility: dirs crate present, pure Rust TF-IDF, closure signature matches Middleware::pre_turn exactly
- Simplicity: no new extension points, no Middleware::merge, no new crates
- Testability: 16 unit tests + integration test scenario, no real LLM needed
- Clarity: FAIL-1 (wiring contradiction) resolved, FAIL-2 (empty guard) resolved

Minor notes (not blocking):
- select_skills tie-breaking unspecified (immaterial for TF-IDF ranking)
- SkillsConfig Default impl not shown (follows ProviderConfig pattern)

Design approved. Next: Explorer hat researches codebase patterns and builds implementation context.

## 2026-03-22 — Explorer research complete

Explored all 7 key source files. Key findings:

1. `MockProvider` and `ErrorProvider` in `turn.rs` test module BOTH implement `Provider` — must update when Step 2 changes the trait signature (add `_system_prompt: Option<&'a str>`)
2. `overlay_from_table` in `config.rs` requires manual extension for `[skills]` section
3. `dirs = "5"` and `tempfile = "3"` already in Cargo.toml — no new deps needed
4. Builder pattern (consuming `mut self`) is consistent across `Conversation` and `Middleware`
5. `#[serde(skip)]` will be the first such field on `Conversation` — standard serde, works correctly
6. `dirs::home_dir()` returns `Option<PathBuf>` — use `.unwrap_or_default()` not `.unwrap()`
7. No `tests/` dir yet — Step 8 creates `ap/tests/skill_injection.rs`

Research written to `.agents/scratchpad/implementation/skill-system/research/` and `context.md`.
Emitting context.ready.

## 2026-03-22 — Plan written

Plan.md written to `.agents/scratchpad/implementation/skill-system/plan.md`.

Key plan decisions:
- 16 unit tests across 4 modules + 1 integration test scenario
- TDD order: Step 1 (types) → Step 2 (provider/turn) → Steps 3-5 (skills module) → Step 6 (config) → Step 7 (wiring) → Step 8 (test + polish)
- All 8 implementation tasks already exist in the task system
- Step 1 (task-1774199280-3ae0) is blocked — Step 2 (task-1774199280-70d5) is the first actionable task
- E2E harness: real CLI + temp skill files, adversarial path (delete skill mid-run)
- Integration test in `tests/skill_injection.rs` — no LLM call needed

Emitting plan.ready.

## 2026-03-22 — build.blocked handled

Build was not actually blocked — Steps 1 and 2 were fully implemented with all tests passing (98/98). Builder emitted build.blocked but work was complete.

Actions taken:
- Verified: `cargo test` all 98 tests pass, Steps 1+2 code is correct
- Closed tasks 3ae0 (Step 1) and 70d5 (Step 2)
- Started task 9440 (Step 3: skills/mod.rs)
- Emitting tasks.ready for Step 3

Active task: task-1774199280-9440 — Step 3: Implement src/skills/mod.rs
Code task file: .agents/scratchpad/implementation/skill-system/tasks/task-03-skill-loader-frontmatter.code-task.md

## 2026-03-22 — Step 3 implemented

### Files changed
- `ap/src/skills/mod.rs` — new file: `Skill` struct, `SkillLoader::new/load`, `parse_skill_file` private helper, `parse_tools_from_frontmatter` private helper
- `ap/src/lib.rs` — added `pub mod skills;`

### TDD cycle
- RED: wrote 5 required tests + 3 extra edge-case tests in mod.rs test module
- GREEN: all 9 tests pass on first compile
- REFACTOR: fixed clippy lint (`option_if_let_else`) in `parse_skill_file`

### Results
- 107/107 tests pass (was 98 before; +9 new)
- `cargo clippy --all-targets -- -D warnings` clean
- `pub mod skills` registered in lib.rs

Next: task-1774199280-b091 (Step 4: select_skills TF-IDF + skills_to_system_prompt)

## 2026-03-22 — Step 4 implemented

### Files changed
- `ap/src/skills/mod.rs` — added `tokenize()`, `select_skills()`, `skills_to_system_prompt()` + 4 new tests

### Key decision
IDF formula: spec says "ln(N / df + 1)" — interpreted as `ln(N/df + 1)` (add 1 to the quotient, NOT divide by `df+1`).
The `ln(N/(df+1))` variant gives IDF=0 for unique terms when N=2 corpus (kills scoring for small skill sets).
Used `(n / doc_freq + 1.0).ln()` with df=0 guard.

### Results
- 111/111 tests pass (was 107; +4 new)
- clippy clean

Next: task-1774199280-c9ab (Step 5: skill_injection_middleware)

## 2026-03-22 — Review of Step 4 (select_skills + skills_to_system_prompt)

### Result: REJECTED

**BUG: `skills_to_system_prompt` — missing newline guard between skills**

`skills_to_system_prompt` formats each skill as `format!("### {}\n{}", name, body)`.
When a skill body does NOT end with `\n`, consecutive skills are run together without any separator:

```
"## Skills\n\n### a\nrust content### b\npython content"
```

This is malformed markdown — the second `###` header is on the same line as the first skill's body.

**Adversarial test that reproduces it:**
```rust
let a = Skill { name: "a".into(), body: "rust content".into(), tools: vec![] };
let b = Skill { name: "b".into(), body: "python content".into(), tools: vec![] };
let out = skills_to_system_prompt(&[&a, &b]);
// out == "## Skills\n\n### a\nrust content### b\npython content"  ← BUG
```

**Fix**: add a newline after body if not already present:
```rust
out.push_str(&format!("### {}\n{}", skill.name, skill.body));
if !skill.body.ends_with('\n') {
    out.push('\n');
}
```

This preserves the existing test (body = "bar\n" → no double newline) while fixing the multi-skill separator case.

The task spec itself says format is `"## Skills\n\n### {name}\n{body}\n"` per skill — the trailing `\n` is always expected.

The existing unit test only covers the single-skill case with a body that already has `\n`. No multi-skill test exists, leaving this bug uncovered.

## 2026-03-22 — Step 5 implemented

### Files changed
- `ap/src/skills/mod.rs` — added:
  - `SkillsConfig` stub struct with `max_injected: usize` + `Default` impl (max 3)
  - `skill_injection_middleware(loader, config)` function returning `impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static`
  - `use crate::types::Conversation` import
  - 2 new tests: `middleware_empty_skills_returns_none` + `middleware_injects_system_prompt`

### TDD cycle
- RED: wrote 2 failing tests, saw compile error (add_user_message → with_user_message typo fixed)
- GREEN: implementation compiled and both tests passed immediately
- REFACTOR: clippy clean

### Results
- 114+5=119 tests pass (was 111; +2 middleware tests + existing tests still green)
- `cargo clippy --all-targets -- -D warnings` clean
- `SkillsConfig` is a stub here; Step 6 will move it to config.rs and wire TOML overlay

## 2026-03-22 — Step 7 implemented

### Files changed
- `ap/src/main.rs` — added imports (`SkillLoader`, `skill_injection_middleware`, `PathBuf`), wired skill middleware into `run_headless` and `run_tui`, added `resolve_skill_dirs()` helper

### Key decisions
- `resolve_skill_dirs` takes `Option<&Vec<PathBuf>>` (not `&Option<Vec<PathBuf>>`) — clippy::ref_option lint
- Default dirs: `~/.ap/skills` + `.ap/skills`, filtered by existence
- Both `run_headless` and `run_tui` use same conditional wiring pattern: `if config.skills.enabled { mw.pre_turn(...) } else { mw }`

### Results
- 119/119 unit tests + 3 integration tests pass
- `cargo clippy --all-targets -- -D warnings` clean
- `cargo build` exits 0

## 2026-03-22 — Step 8 implemented

### Files created
- `ap/tests/skill_injection.rs` — integration test: 4 acceptance criteria covered in `skill_pipeline_end_to_end`
  - AC-1: Later-wins override (shared.md: project beats global)
  - AC-2: TF-IDF selects git skill, excludes docker for git query
  - AC-3: Middleware injects system_prompt containing "git"
  - AC-4: Middleware returns None for empty conversation

### Results
- 119+2+3+1 = 125 total tests pass (was 124 before; +1 integration test)
- `cargo clippy --all-targets -- -D warnings` — 0 warnings
- `cargo build --release` — exits 0
- All public items in `src/skills/mod.rs` already had `///` doc comments from prior steps

All 8 steps of skill system complete.
