# Tool Discovery — Implementation Progress

*Last updated: 2026-03-22*

## Current Step: 5

## Active Wave

| Task ID | Key | Code Task File |
|---|---|---|
| task-1774202075-2cf0 | pdd:tool-discovery:step-05:wire-main | tasks/task-05-wire-main.code-task.md |

## Step Status

| Step | Title | Status |
|---|---|---|
| 1 | Discovery types + TOML serde | ✅ complete |
| 2 | discover() pure function | ✅ complete |
| 3 | ShellTool implementation | ✅ complete |
| 4 | System prompt threading | ✅ complete |
| 5 | Wire discovery into main.rs | ⬜ pending |

## Step 2: discover() pure function — COMPLETE (2026-03-22)

### TDD Cycle
**RED**: Wrote 12 new `discover()` tests using `tempfile::TempDir` — empty dir, valid tools.toml, malformed tools.toml, partial tools.toml (whole-file skip), skill file extraction, alphabetical ordering, duplicate detection (tools.toml wins, a.toml wins over b.toml), system_prompt accumulation, param insertion order, malformed skill file.

**GREEN**: Implemented `pub fn discover(root: &Path) -> DiscoveryResult` in `ap/src/discovery/mod.rs`:
- Reads `tools.toml` first (silent skip if missing, warning if malformed)
- Reads `.ap/skills/*.toml` sorted by filename (silent skip if no dir)
- Deduplicates via `HashSet<String>` (first-wins)
- `add_tool()` helper keeps the loop DRY
- No `unwrap()` or `expect()` in production code path

**REFACTOR**: Fixed clippy issues:
- `match` on `read_dir` → `map_or_else` (clippy::option_if_let_else)
- `|s| s.as_str()` → `String::as_str` (redundant_closure)
- Added `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` to test module

### Build Gate
- `cargo test`: 17/17 discovery tests pass, all 5 other tests pass
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo build`: clean

### Files Changed
- `ap/src/discovery/mod.rs` — added `discover()` function + 12 new tests

## Step 5: Wire main.rs — 2026-03-22

### Changes
- `ap/src/main.rs`: added `use ap::discovery::discover` and `use ap::tools::ShellTool`
- `run_headless`: project_root → discover() → print warnings → register ShellTools (before middleware) → build system_prompt → conv.with_system_prompt()
- `run_tui`: same discovery block → register ShellTools before `Arc::new(tools)` → apply system_prompt to initial Conversation

### Verification
- `cargo build --package ap` ✅
- `cargo test --package ap` (128 tests) ✅
- `cargo clippy --package ap -- -D warnings` ✅
- E2E: tools.toml with "greet" tool — Claude invoked it ✅
- E2E: malformed skill TOML → `ap: ...` warning on stderr, no crash ✅
