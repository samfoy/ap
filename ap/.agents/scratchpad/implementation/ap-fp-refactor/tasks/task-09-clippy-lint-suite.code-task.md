---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: Clippy Lint Suite — Enforce Functional Style

## Description
Add a clippy lint configuration that enforces functional style at compile time. Apply workspace-level lints in `Cargo.toml` and crate-level gates in `main.rs`. The CI gate is `cargo clippy --all-targets -- -D warnings` exiting clean.

## Background
This is the polish step. The codebase should be so clean that the lint suite enforces the FP conventions going forward: no `unwrap`, no `expect`, no `panic`, pedantic style. This is applied last so it doesn't block earlier implementation steps.

## Reference Documentation
**Required:**
- Design amendment in ap/.ralph/agent/scratchpad.md (Clippy lint suite section)
- ap/Cargo.toml (workspace lints section to add)
- ap/src/main.rs (crate-level gates to add)

## Technical Requirements

1. Add to `ap/Cargo.toml` (workspace-level lints):
   ```toml
   [workspace.lints.rust]
   unsafe_code = "forbid"

   [workspace.lints.clippy]
   unwrap_used = "deny"
   expect_used = "deny"
   panic = "deny"
   needless_pass_by_ref_mut = "deny"
   option_if_let_else = "warn"
   map_unwrap_or = "warn"
   manual_let_else = "warn"
   redundant_closure_for_method_calls = "warn"
   explicit_iter_loop = "warn"
   pedantic = "warn"
   ```

2. Add crate-level gates to top of `ap/src/main.rs`:
   ```rust
   #![deny(unsafe_code)]
   #![deny(clippy::unwrap_used)]
   #![deny(clippy::expect_used)]
   #![warn(clippy::pedantic)]
   #![allow(clippy::module_name_repetitions)]
   #![allow(clippy::must_use_candidate)]
   ```

3. Fix ALL clippy violations raised by the new lint suite across all source files:
   - Replace `.unwrap()` with `?` or `unwrap_or_else(|e| ...)` as appropriate
   - Replace `.expect("...")` with `anyhow::Context::context()` or proper error handling
   - Fix pedantic warnings (missing docs on public items can use `#[allow(missing_docs)]` at crate level if noisy)
   - Add `#[allow(clippy::module_name_repetitions)]` where needed (crate-level already covers it)

4. Ensure `cargo clippy --all-targets -- -D warnings` exits 0

5. Ensure `cargo test` still passes after all fixes

6. Update README.md "Development" section to note: `cargo clippy --all-targets -- -D warnings` is a required check

## Dependencies
- Task 07: AgentLoop deleted (no dead-code lint noise from unused AgentLoop)
- Task 08: README.md update (will add clippy note here too)

## Implementation Approach
1. Add workspace lints to Cargo.toml
2. Run `cargo clippy --all-targets 2>&1 | head -100` to see initial violation list
3. Fix violations file by file, starting with the most critical (unwrap/expect/panic)
4. Re-run clippy until clean
5. Run `cargo test` to confirm no regressions
6. Commit with message: `chore: add clippy lint suite enforcing functional style`

## Acceptance Criteria

1. **Workspace lints active**
   - Given `ap/Cargo.toml`
   - When examining the `[workspace.lints.clippy]` section
   - Then `unwrap_used = "deny"`, `expect_used = "deny"`, and `pedantic = "warn"` are present

2. **Crate-level gates in main.rs**
   - Given `ap/src/main.rs`
   - When reading the top of the file
   - Then `#![deny(clippy::unwrap_used)]` and `#![warn(clippy::pedantic)]` are present

3. **Clippy exits clean**
   - Given the implementation is complete
   - When running `cargo clippy --all-targets -- -D warnings`
   - Then exit code is 0

4. **All tests still pass**
   - Given the lint suite is applied
   - When running `cargo test`
   - Then all tests pass

5. **README notes clippy as required check**
   - Given README.md
   - When reading the Development section
   - Then `cargo clippy --all-targets -- -D warnings` is documented as a required check

## Metadata
- **Complexity**: Medium
- **Labels**: clippy, lint, fp-refactor, polish
- **Required Skills**: Rust, clippy
