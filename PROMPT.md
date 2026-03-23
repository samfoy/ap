`PROMPT.md` written to `/Users/sam.painter/Projects/ap/PROMPT.md`. Here's a summary of what it contains:

---

**Vision** — First-class Amazon toolchain support: profile detection, `aws` CLI tool, Ada credential retry, Brazil workspace awareness, TUI profile indicator. All implemented as pure functions / `impl Tool` / thin async wrappers — no global state.

**8 ordered implementation steps**, each independently compilable:

| Step | Files | What it adds |
|------|-------|--------------|
| 1 | `config.rs` | `AwsConfig` struct + `AppConfig.aws` field + overlay |
| 2 | `src/aws/profile.rs` | `detect_aws_profile[_from]()` pure function |
| 3 | `src/tools/aws.rs` | `AwsTool` implementing `Tool` |
| 4 | `src/aws/ada.rs` | `is_credential_error`, `run_ada_refresh`, `turn_with_ada_retry` |
| 5 | `src/brazil/mod.rs` | `BrazilContext`, detection, log finding, system prompt |
| 6 | `src/discovery/mod.rs` | Brazil prompt injected into `DiscoveryResult` |
| 7 | `src/tui/{mod,ui}.rs` | `aws_profile` field + `format_aws_segment()` in status bar |
| 8 | `src/main.rs` | Wire `AwsTool`, `turn_with_ada_retry`, profile → TUI |

**Acceptance criteria** cover all 8 areas with specific function signatures and test assertions. The loop exits on `cargo build && cargo test && cargo clippy -- -D warnings` all passing clean.