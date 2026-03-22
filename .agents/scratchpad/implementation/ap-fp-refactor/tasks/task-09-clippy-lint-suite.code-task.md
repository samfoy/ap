---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: Clippy Lint Suite — Enforce Functional Style

## Description
Add a clippy lint configuration to `ap/Cargo.toml` that enforces functional style at compile time. Add crate-level gates in `src/main.rs`. Fix all violations. The goal is `cargo clippy --all-targets -- -D warnings` exits 0 with the full lint suite active.

## Background
Per design amendment (Sam, 09:13 PDT 2026-03-22): The refactored codebase should have compile-time enforcement of functional style conventions — no `unwrap`, no `expect`, no `unsafe`, prefer functional iterator patterns over imperative loops.

## Reference Documentation
**Required:**
- Design amendment in scratchpad (clippy lint suite section)
- ap/Cargo.toml — current workspace lint configuration

## Technical Requirements
1. Add to `ap/Cargo.toml` under `[workspace.lints.clippy]`:
   - `unwrap_used = "deny"`
   - `expect_used = "deny"`
   - `panic = "deny"`
   - `needless_pass_by_ref_mut = "deny"`
   - `option_if_let_else = "warn"`
   - `map_unwrap_or = "warn"`
   - `manual_let_else = "warn"`
   - `redundant_closure_for_method_calls = "warn"`
2. Add to `[workspace.lints.rust]`:
   - `unsafe_code = "forbid"`
3. Add crate-level gates to `src/main.rs`:
   - `#![deny(unsafe_code)]`
   - `#![deny(clippy::unwrap_used)]`
   - `#![deny(clippy::expect_used)]`
   - `#![warn(clippy::pedantic)]`
   - `#![allow(clippy::module_name_repetitions)]`
   - `#![allow(clippy::must_use_candidate)]`
4. Fix ALL violations revealed by the lint suite — no `#[allow]` suppressions unless genuinely unavoidable (document each one)
5. `cargo clippy --all-targets -- -D warnings` must exit 0
6. All 98 tests must continue to pass

## Dependencies
- Steps 01–08 completed

## Implementation Approach
1. Add lint config to `Cargo.toml`
2. Run `cargo clippy --all-targets -- -D warnings` to see all violations
3. Fix violations systematically:
   - Replace `unwrap()` / `expect()` with `?`, `unwrap_or`, `unwrap_or_else`, or proper error propagation
   - Replace imperative loops with iterator chains where natural
   - Fix any pedantic warnings that are legitimate
4. Add `#[allow]` only for unavoidable false positives (document each)
5. Run full test suite to confirm no regressions

## Acceptance Criteria

1. **Lint Config Present**
   - Given `ap/Cargo.toml` is read
   - When the `[workspace.lints.clippy]` section is found
   - Then it contains `unwrap_used = "deny"`, `expect_used = "deny"`, and `panic = "deny"`

2. **Crate Gates Present**
   - Given `src/main.rs` is read
   - When the top of the file is inspected
   - Then it contains `#![deny(clippy::unwrap_used)]` and `#![warn(clippy::pedantic)]`

3. **Clippy Clean**
   - Given the lint suite is active
   - When running `cargo clippy --all-targets -- -D warnings`
   - Then it exits 0 with zero warnings or errors

4. **Tests Still Pass**
   - Given the lint suite is active and violations are fixed
   - When running `cargo test`
   - Then all tests pass

5. **No unsafe code**
   - Given `unsafe_code = "forbid"` is set
   - When running `cargo build --release`
   - Then zero `unsafe` blocks exist in the codebase

## Metadata
- **Complexity**: Medium
- **Labels**: quality, clippy, linting
- **Required Skills**: Rust, Clippy
