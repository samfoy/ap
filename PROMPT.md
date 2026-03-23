Now I have a thorough picture of the codebase. Let me write the PROMPT.md.Here is the content written to `PROMPT.md`:

---

The document covers:

**Vision** — frames the goal clearly: every headless run becomes a first-class session, saved to `~/.ap/sessions/`, resumable with `--session`.

**Technical context** — reproduces the exact Rust types and signatures the implementor must work with (`Conversation`, `SessionStore`, the current `run_headless` signature), so there's no guessing.

**Slug generation spec** — a precise, testable algorithm (pure function, no I/O, deterministic inputs) with a worked-example table covering edge cases (punctuation, leading spaces, truncation).

**5 ordered implementation steps**, each independently compilable:
1. Pure `prompt_slug` function + unit tests in `src/session/mod.rs`
2. Wire auto-slug into `run_headless` in `src/main.rs`, always creating a `SessionStore` and always saving after success
3. Integration test (`headless_turn_saves_session_to_store`) using the existing `MockProvider` pattern
4. Clap help-text update for `--session` (docs only, zero risk)
5. Backlog housekeeping (`[ ]` → `[x]`)

**12 explicit acceptance criteria** with a table — each maps to a specific observable fact the loop can verify mechanically before emitting `LOOP_COMPLETE`.

**Constraints** reinforce the project's style rules: no new deps, no `chrono`, pure functions for slug logic, `clippy::unwrap_used` stays denied.