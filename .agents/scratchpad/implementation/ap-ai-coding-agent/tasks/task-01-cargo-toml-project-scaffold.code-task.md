---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Cargo.toml + Project Scaffold

## Description
Set up the `ap` Rust project with all dependencies declared in `Cargo.toml`, a working `main.rs` that parses `--version` via clap and prints it, plus an example config file and `.gitignore`. The binary must compile cleanly and `ap --version` must print `ap 0.1.0`.

## Background
This is the foundation step. All subsequent steps build on top of this scaffold. Getting the dependency graph right now avoids churn later — every crate used across all 12 steps must be declared here.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (overall strategy)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Create `ap/Cargo.toml` with all required dependencies declared:
   - `ratatui`, `crossterm` — TUI
   - `tokio` (features: full) — async runtime
   - `clap` (features: derive) — CLI
   - `reqwest` (features: json) — HTTP client (reserved for future provider HTTP option)
   - `serde`, `serde_json` (features: derive) — serialization
   - `toml` — config parsing
   - `aws-sdk-bedrockruntime`, `aws-config`, `aws-credential-types` — AWS Bedrock
   - `futures` — async utilities (BoxFuture, BoxStream)
   - `anyhow`, `thiserror` — error handling
   - `rhai = { version = "1", features = ["sync"] }` — scripting extensions
   - `libloading` — dylib loading
   - `tempfile` — temp files for hooks tests
   - `dirs` — home directory resolution
   - `uuid` (features: v4) — session IDs
2. Create `ap/src/main.rs` that uses clap derive API to define `--version` flag and prints `ap 0.1.0` on invocation
3. Create `ap/ap.toml.example` with all config sections documented with comments
4. Create `ap/.gitignore` with standard Rust ignores (`/target`, `Cargo.lock` optional for binaries)
5. The binary must have name `ap` in Cargo.toml

## Dependencies
- No prior tasks — this is the first step

## Implementation Approach
1. TDD: Write a simple test verifying the binary compiles (acceptance test: `cargo build --release` exits 0)
2. Create minimal `main.rs` first, then add dependencies incrementally until all compile
3. Verify: `cargo build --release` succeeds with zero warnings
4. Verify: `./target/release/ap --version` prints version string

## Acceptance Criteria

1. **Binary Compiles**
   - Given the `ap/` project directory with `Cargo.toml`
   - When running `cargo build --release`
   - Then the build succeeds with zero errors and zero warnings

2. **Version Flag Works**
   - Given the compiled binary at `target/release/ap`
   - When running `./target/release/ap --version`
   - Then stdout contains `ap 0.1.0` and exit code is 0

3. **All Dependencies Declared**
   - Given `Cargo.toml`
   - When running `cargo check`
   - Then all crates resolve without errors (no version conflicts)

4. **Example Config Exists**
   - Given the project root
   - When reading `ap.toml.example`
   - Then it contains `[provider]`, `[tools]`, `[hooks]`, and `[extensions]` sections with inline documentation comments

## Metadata
- **Complexity**: Low
- **Labels**: scaffold, cargo, dependencies
- **Required Skills**: Rust, Cargo dependency management
