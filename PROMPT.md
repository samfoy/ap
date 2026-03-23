Here's what the PROMPT.md covers and why each decision was made:

**Vision** — sets out the two concrete problems with the current code: wrong path (`ap.toml` → `.ap/config.toml`) and wrong discovery (CWD-only → walk to git root).

**Codebase orientation** — gives the Ralph loop exactly the two files it needs to touch and the exact existing API surface it must not break.

**Technical requirements** — specifies all three new pieces with exact Rust signatures:
- `discover_project_config(start: &Path) -> Option<PathBuf>` — pure, no panics, walks to git root
- `AppConfig::load(skip_project_config: bool)` — clean signature change that threads through the new flag
- Three new `Args` fields with their wiring in `main()`

**Ordered steps** — each step is independently compilable:
1. Discovery function + tests only (no touching `load()` or `main.rs`)
2. `load()` signature change only (no touching `main.rs`)
3. CLI flags only
4. Integration smoke test

This ordering means the loop can validate each step before proceeding, and a failure in step 3 can't corrupt work done in step 1.

**Acceptance criteria (AC1–AC10)** — covers build cleanliness, test counts, the no-`ap.toml`-string requirement, clippy lints, and the functional-first style rule against `mut`. AC7 specifically catches the hardcoded `"ap.toml"` string so Ralph can't accidentally leave the old path in place.