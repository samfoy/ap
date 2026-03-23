Now I have a thorough understanding of the codebase. Let me write the PROMPT.md:Here is the `PROMPT.md` content written to `~/Projects/ap/ap/PROMPT.md`:

---

The document is structured as follows:

**Vision** — explains the *why*: transient failures shouldn't surface as raw errors; the user should see live retry progress and the agent should heal itself automatically, all without violating the functional-first design.

**Technical Requirements** cover five concrete contracts:
1. `src/retry.rs` — new pure-function module with `RetryConfig`, `RetryKind`, `backoff_delay`, `parse_retry_after`, `classify_error`, and `explicit_delay`.
2. Four new `ProviderError` variants (`Http`, `Network`, `InvalidRequest`, `RateLimited`) that give `classify_error` structured input instead of string-matching.
3. New `TurnEvent::Retrying` variant carrying all display data the TUI needs.
4. `turn_with_retry` public function — the retry loop, pure except for `tokio::time::sleep`.
5. TUI state: `RetryStatus` struct, `retry_status` field, status-bar render helper `format_retry_segment`.

**Six ordered steps**, each independently compilable:
- Step 1: config struct only — zero logic, establishes the TOML wiring.
- Step 2: pure math functions (`backoff_delay`, `parse_retry_after`).
- Step 3: error classification (`classify_error`, new `ProviderError` variants).
- Step 4: TUI plumbing (`Retrying` event → `retry_status` → status bar).
- Step 5: `turn_with_retry` core loop with sleep, event emission, Retry-After guard.
- Step 6: call-site wiring in `handle_submit` and `run_headless`, plus `ap.toml.example` update.

**12 acceptance criteria** map one-to-one to testable behaviours, ending with the `LOOP_COMPLETE` sentinel.