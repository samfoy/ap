---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Implement src/skills/mod.rs — Skill and SkillLoader

## Description
Create the new `src/skills/mod.rs` module with the `Skill` struct, `SkillLoader` struct, and frontmatter parsing logic. Register the module in `src/lib.rs`. `SkillLoader::load()` reads all `.md` files from the configured directories, parses optional YAML-lite frontmatter, and merges by skill name with later-wins semantics.

## Background
Skills are plain Markdown files stored in `~/.ap/skills/` (global) and `./.ap/skills/` (project). The frontmatter is optional and only the `tools:` key is parsed (as a list). Project skills override global skills by name. `load()` is called every turn so file changes take effect without restarting.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/skill-system/design.md (Sections 2 FR-3/FR-4, Section 4.3, Appendix C.1)

**Additional References:**
- .agents/scratchpad/implementation/skill-system/context.md (module registration in lib.rs, no new crates needed)
- .agents/scratchpad/implementation/skill-system/plan.md (Step 3)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Create `src/skills/mod.rs` with:
   - `pub struct Skill { pub name: String, pub body: String, pub tools: Vec<String> }`
   - `pub struct SkillLoader { dirs: Vec<PathBuf> }`
   - `impl SkillLoader { pub fn new(dirs: Vec<PathBuf>) -> Self; pub fn load(&self) -> Vec<Skill> }`
2. `load()` iterates `self.dirs` in order; for each dir, reads all `.md` files; inserts into a `IndexMap`/`BTreeMap` keyed by skill name — later entries overwrite earlier ones (later-wins)
3. Frontmatter parser: scan lines; if first line is `---`, parse until next `---`; parse `tools: [a, b]` with simple string splitting; body is everything after the closing `---`
4. No new crates — use only `std` (plus `dirs` crate already in Cargo.toml for Step 6)
5. Register `pub mod skills;` in `src/lib.rs`
6. `clippy::unwrap_used` and `clippy::expect_used` denied — use `?`, `ok()`, `unwrap_or_default()` etc.

## Dependencies
- Task 01 (Step 1): `Conversation` type needed for later steps; `src/lib.rs` patterns established
- Task 02 (Step 2): module structure established; `src/lib.rs` registration pattern confirmed

## Implementation Approach
1. **RED**: Write 5 failing tests in `src/skills/mod.rs`:
   - `skill_loader_empty_dirs`: `SkillLoader::new(vec![])` → `.load()` → empty vec
   - `skill_loader_loads_skills`: tempdir with one `foo.md` → returns Skill with name="foo"
   - `skill_loader_later_dir_overrides`: two tempdirs, both have `shared.md` with different bodies → second wins
   - `skill_frontmatter_tools_parsed`: file with `---\ntools: [bash, read]\n---\nbody` → `tools==["bash","read"]`, `body=="body\n"` (or `"body"`)
   - `skill_no_frontmatter_full_body`: file with no `---` → `body == entire file content`, `tools == []`
2. **GREEN**: Implement `SkillLoader::new`, `load()`, frontmatter parser
3. **REFACTOR**: Extract frontmatter parsing into a private `parse_skill_file(content: &str) -> (Vec<String>, String)` helper

## Acceptance Criteria

1. **Empty dirs returns empty vec**
   - Given `SkillLoader::new(vec![])`
   - When `.load()` is called
   - Then returns an empty `Vec<Skill>`

2. **Single file loads correctly**
   - Given a directory with one `foo.md` file containing `"# Hello"`
   - When `SkillLoader::new(vec![dir]).load()` is called
   - Then returns one `Skill` with `name=="foo"` and `body=="# Hello"`

3. **Later directory overrides same-name skill**
   - Given two directories, both with `shared.md`, first has body "GLOBAL", second has body "PROJECT"
   - When `SkillLoader::new(vec![global, project]).load()` is called
   - Then the returned skill has `body=="PROJECT"`

4. **Frontmatter parsed correctly**
   - Given a file with `---\ntools: [bash, read]\n---\nbody text`
   - When loaded
   - Then `skill.tools == ["bash", "read"]` and `skill.body` contains `"body text"`

5. **No frontmatter uses full content as body**
   - Given a file with no `---` delimiters
   - When loaded
   - Then `skill.body == entire file content` and `skill.tools == []`

6. **Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test -- skill_loader`
   - Then all 5 tests pass

## Metadata
- **Complexity**: Medium
- **Labels**: skills, loader, frontmatter, parsing
- **Required Skills**: Rust, std::fs, string parsing
