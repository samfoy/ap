Here is the complete `PROMPT.md` content:

---

The document covers:

**Vision** — why retry matters and what the user experience should be.

**Architecture constraints** — `turn()` signature is frozen, retry logic is isolated in `src/retry.rs`, no sleeping inside the provider, `Conversation` stays immutable.

**Six ordered implementation steps**, each independently compilable:

1. **`RetryConfig`** in `config.rs` — the new `[retry]` TOML section with the layered-merge pattern the codebase already uses for `[context]`, `[skills]`, etc.

2. **Extend `ProviderError`** — add `RateLimited` and `ServerError` typed variants with `is_retryable()` / `retry_after_hint()`, plus `classify_aws_error()` in `bedrock.rs` that maps SDK string errors to those variants.

3. **`TurnEvent::Retrying`** — minimal new variant; adds exhaustive `match` arms as no-ops before the later wiring steps so the project stays green.

4. **`src/retry.rs` pure engine** — `compute_delay()`, `RetryDecision`, and `should_retry()` with no I/O; fully testable without async, using `tokio::time::pause()`.

5. **Wire into `turn_loop`** — extracts `drive_stream()` helper, wraps it in the retry loop with `TurnEvent::Retrying` emission and accumulator reset; uses `FallibleProvider` test double.

6. **TUI integration** — `retry_status: Option<(u32, u32)>` field, event handler, status bar rendering, and headless stderr output.

**12 acceptance criteria** map precisely to each capability, ending with a clean `cargo build --release` and zero test failures as the `LOOP_COMPLETE` gate.