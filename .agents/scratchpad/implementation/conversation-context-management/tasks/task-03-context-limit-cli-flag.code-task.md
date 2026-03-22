---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: --context-limit CLI Flag

## Description
Add `--context-limit <TOKENS>` as an optional CLI argument to `ap`. When provided, it overrides `config.context.limit`. The compression is not wired yet — this step is purely about accepting the flag and propagating the value through the config.

## Background
`ap/src/main.rs` uses the `clap` crate (derive macro) for argument parsing. The `Args` struct is the top-level clap struct. After `AppConfig::load()`, the code overlays some CLI args onto the config (e.g., model override). The same pattern should be followed here: parse the flag → if `Some`, override `config.context.limit`.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/conversation-context-management/design.md`

**Additional References:**
- `.agents/scratchpad/implementation/conversation-context-management/context.md` (codebase patterns, especially how Args overlays config)
- `.agents/scratchpad/implementation/conversation-context-management/plan.md` (overall strategy)

**Note:** You MUST read the design document before beginning implementation. Also read `ap/src/main.rs` in full before making changes.

## Technical Requirements
1. Add `#[arg(long)] pub context_limit: Option<u32>` to the `Args` struct in `main.rs`
2. After `AppConfig::load()`, add: `if let Some(limit) = args.context_limit { config.context.limit = Some(limit); }`
3. Store the limit locally (a `let context_limit = config.context.limit;` binding) for future use in Steps 6 and 7
4. No other behavioral changes — `turn()` is unaffected

## Dependencies
- Task 02 (`config.context.limit` field must exist)

## Implementation Approach
1. Add the clap field to `Args`
2. Add the overlay after `AppConfig::load()`
3. Add the local binding
4. `cargo build` — zero warnings, zero errors
5. Run `cargo test` — all existing tests pass (no new tests for this step; the flag is a thin passthrough)

## Acceptance Criteria

1. **CLI Flag Accepted**
   - Given the binary is compiled
   - When running `ap --help`
   - Then `--context-limit` appears in the help output

2. **CLI Override Works**
   - Given `config.context.limit` is `None` from the config file
   - When `--context-limit 50000` is passed on the command line
   - Then `config.context.limit` equals `Some(50000)` after the overlay step

3. **No-Flag Preserves Config**
   - Given `--context-limit` is not passed
   - When the program starts
   - Then `config.context.limit` is whatever was in the config file (or `None` by default)

4. **All Existing Tests Pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all pre-existing tests pass and `cargo build` produces zero warnings

## Metadata
- **Complexity**: Low
- **Labels**: context-management, cli, clap
- **Required Skills**: Rust, clap derive macros
