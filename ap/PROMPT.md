# PROMPT.md — Retry with Exponential Backoff

## Vision

When a provider call fails due to a transient condition (rate-limiting, server
overload, network blip, SSE stream drop), the agent should automatically retry
rather than surfacing a raw error to the user. Retries use exponential backoff
to avoid thundering-herd problems, respect `Retry-After` headers from the
server, and are fully configurable. Non-retryable errors (auth failures, bad
requests, quota exceeded beyond `max_delay_ms`) fail immediately with a clear
message. The TUI status bar shows live retry progress (`retrying (2/3)...`) so
the user is never left wondering why the agent is silent.

The implementation must stay true to the project's functional-first philosophy:
- No mutation of shared state; retry logic is a pure transform on the provider
  call site.
- The retry engine is a standalone module (`src/retry.rs`) with zero TUI or
  provider coupling.
- `TurnEvent::Retrying` carries all needed display data; the TUI consumes it
  like any other event — no special-casing in the turn loop.
- All new public functions are pure (no side-effects beyond `tokio::time::sleep`)
  and are independently unit-testable with a mock clock/provider.

---

## Technical Requirements

### 1. New types and signatures

#### `src/retry.rs` (new module)

```rust
use std::time::Duration;
use crate::provider::ProviderError;

/// Classification of whether a ProviderError can be retried.
#[derive(Debug, Clone, PartialEq)]
pub enum RetryKind {
    /// Transient — worth retrying (HTTP 429, 5xx, stream drop, timeout).
    Transient,
    /// Permanent — fail immediately (4xx auth/bad-request, quota > max_delay).
    Permanent,
}

/// Decide whether `err` is worth retrying.
///
/// Pure function — no I/O.
pub fn classify_error(err: &ProviderError) -> RetryKind { ... }

/// Configuration for the retry engine (loaded from `[retry]` TOML table).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetryConfig {
    pub enabled: bool,
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            base_delay_ms: 2000,
            max_delay_ms: 60_000,
        }
    }
}

/// Compute the delay for attempt `n` (0-indexed), applying exponential
/// backoff capped at `config.max_delay_ms`.
///
/// Formula: min(base_delay_ms * 2^n, max_delay_ms)
///
/// Pure function — no I/O.
pub fn backoff_delay(config: &RetryConfig, attempt: u32) -> Duration { ... }

/// Parse a `Retry-After` header value (seconds as integer or HTTP-date string).
/// Returns `None` when the value cannot be parsed.
///
/// Pure function — no I/O.
pub fn parse_retry_after(value: &str) -> Option<Duration> { ... }
```

#### Extended `ProviderError` (in `src/provider/mod.rs`)

Add variants that carry enough structured information for `classify_error` to
work without string-matching:

```rust
pub enum ProviderError {
    // existing variants kept as-is ...
    #[error("AWS error: {0}")]
    Aws(String),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    // NEW variants:
    /// HTTP-level error with a status code. 429 and 5xx are retryable.
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String, retry_after: Option<Duration> },

    /// Network-level failure (timeout, connection reset, stream EOF). Always retryable.
    #[error("network error: {0}")]
    Network(String),

    /// Request was structurally invalid; never retryable.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// Rate-limited with an explicit retry delay from the server.
    #[error("rate limited: retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
}
```

> **Note on existing `Aws(String)` variant:** The current Bedrock provider maps
> *all* SDK errors to `ProviderError::Aws(String)`. Keep that variant for
> backward compat; `classify_error` will inspect the string content as a
> fallback for errors that do not yet use the structured variants. New code
> should prefer the structured variants.

#### Extended `TurnEvent` (in `src/types.rs`)

```rust
pub enum TurnEvent {
    // ... existing variants unchanged ...

    /// The provider failed transiently; the agent is about to retry.
    Retrying {
        attempt: u32,      // 1-indexed ("attempt 1 of 3")
        max_retries: u32,
        delay_ms: u64,     // actual delay that will be slept
        reason: String,    // human-readable error that triggered the retry
    },
}
```

#### `turn_with_retry` in `src/turn.rs`

```rust
/// Execute one agent turn with automatic retry on transient provider errors.
///
/// Wraps `turn()`. On a transient `ProviderError`, emits `TurnEvent::Retrying`,
/// sleeps the computed backoff, then calls `turn()` again. On a permanent error
/// or after exhausting retries, returns `Err`.
///
/// `event_tx` is an optional channel for emitting `TurnEvent::Retrying` events
/// to the TUI *before* the sleep; callers that don't need live status may pass
/// `None`.
pub async fn turn_with_retry(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
    retry_config: &RetryConfig,
    event_tx: Option<&mpsc::Sender<TurnEvent>>,
) -> Result<(Conversation, Vec<TurnEvent>)> { ... }
```

### 2. Config integration

`AppConfig` gains a `retry: RetryConfig` field (TOML section `[retry]`).
`overlay_from_table` is extended to handle the `retry` key, following the same
fine-grained overlay pattern already used for `context`, `skills`, etc.

The `ap.toml.example` file is updated with a commented-out `[retry]` section.

### 3. TUI wiring

- `TuiApp::handle_ui_event` handles the new `TurnEvent::Retrying` variant:
  - Sets `self.retry_status = Some(RetryStatus { attempt, max_retries })`.
  - Sets `self.is_waiting = true` (already true; this is a no-op).
- `TuiApp` gains `pub retry_status: Option<RetryStatus>`.
- `RetryStatus` is a plain struct:
  ```rust
  pub struct RetryStatus {
      pub attempt: u32,
      pub max_retries: u32,
  }
  ```
- `TurnEvent::TurnEnd` and `TurnEvent::Error` clear `retry_status` (set to `None`).
- The status bar render function (`tui/ui.rs`) appends `retrying (N/M)...` to
  the status text when `app.retry_status` is `Some`.

### 4. Call-site wiring

`handle_submit` in `tui/mod.rs` and `run_headless` in `main.rs` are updated to
call `turn_with_retry` instead of `turn` directly, passing the `RetryConfig`
from `conv.config.retry` and the `ui_tx` sender (TUI path) or `None` (headless
path).

---

## Ordered Implementation Steps

Each step must leave the project in a compilable, test-passing state before
moving to the next.

---

### Step 1 — `RetryConfig` struct + config integration

**Files changed:** `src/retry.rs` (new), `src/config.rs`, `src/lib.rs`,
`ap.toml.example`

1. Create `src/retry.rs` containing only `RetryConfig` with `Default`,
   `Serialize`, `Deserialize`, `Debug`, `Clone`.  No logic yet.
2. Add `pub mod retry;` to `src/lib.rs`.
3. Add `pub retry: RetryConfig` to `AppConfig`; derive it with `#[serde(default)]`.
4. Extend `overlay_from_table` with a `retry` branch mirroring the `context`
   branch (overlay only keys present in the TOML table).
5. Add a commented `[retry]` section to `ap.toml.example`.

**Tests to add in `src/retry.rs`:**
- `retry_config_defaults` — `enabled=true`, `max_retries=3`,
  `base_delay_ms=2000`, `max_delay_ms=60000`.
- `retry_config_toml_overlay` — parse a TOML snippet, assert non-default values
  land on `AppConfig`.
- `retry_config_partial_overlay_preserves_defaults` — only `max_retries=5` in
  TOML; `base_delay_ms` etc. keep defaults.
- `retry_config_disabled` — `enabled = false` round-trips.

**Compile check:** `cargo test --lib` must pass with zero new warnings.

---

### Step 2 — `backoff_delay` and `parse_retry_after` pure functions

**Files changed:** `src/retry.rs`

1. Add `backoff_delay(config: &RetryConfig, attempt: u32) -> Duration`.
   Formula: `min(base_delay_ms * 2^attempt, max_delay_ms)`.  Use saturating
   arithmetic (`u64::saturating_mul`, `u64::saturating_pow`) to avoid overflow
   on large `attempt` values.
2. Add `parse_retry_after(value: &str) -> Option<Duration>`.
   - If the string parses as a `u64`, return `Duration::from_secs(n)`.
   - If the string matches an HTTP-date (`%a, %d %b %Y %H:%M:%S GMT`), compute
     seconds until that instant using `std::time::SystemTime`; return `None` if
     the date is in the past.
   - Otherwise return `None`.
   - Keep the HTTP-date parsing minimal: use `std::time::SystemTime` and manual
     RFC-2822 parsing — **do not add a new crate**.

**Tests to add:**
- `backoff_delay_attempt_0` → `base_delay_ms`.
- `backoff_delay_attempt_1` → `2 * base_delay_ms`.
- `backoff_delay_attempt_2` → `4 * base_delay_ms`.
- `backoff_delay_capped_at_max` — attempt large enough that uncapped value
  exceeds `max_delay_ms`; result must equal `max_delay_ms`.
- `backoff_delay_no_overflow_on_huge_attempt` — attempt=100 must not panic.
- `parse_retry_after_integer_seconds` → `Duration::from_secs(30)`.
- `parse_retry_after_zero` → `Duration::ZERO`.
- `parse_retry_after_invalid_returns_none` — `"bananas"` → `None`.

**Compile check:** `cargo test --lib` must pass.

---

### Step 3 — `ProviderError` new variants + `classify_error`

**Files changed:** `src/provider/mod.rs`, `src/retry.rs`

1. Add `Http`, `Network`, `InvalidRequest`, `RateLimited` variants to
   `ProviderError` (see signatures above). Add `use std::time::Duration;` to
   `src/provider/mod.rs`.
2. Implement `classify_error(err: &ProviderError) -> RetryKind` in
   `src/retry.rs`:
   - `ProviderError::Http { status, .. }`: `429` or `500..=599` → `Transient`; all other 4xx → `Permanent`.
   - `ProviderError::Network(_)` → `Transient`.
   - `ProviderError::RateLimited { .. }` → `Transient`.
   - `ProviderError::InvalidRequest(_)` → `Permanent`.
   - `ProviderError::Aws(msg)` — inspect string:
     - contains `"throttling"` (case-insensitive) or `"too many requests"` → `Transient`.
     - contains `"timeout"` or `"connection"` → `Transient`.
     - all other `Aws` errors → `Permanent` (conservative default).
   - `ProviderError::ParseError(_)` → `Permanent`.
   - `ProviderError::Serialization(_)` → `Permanent`.
3. Add `RetryKind` to `src/retry.rs` (`Debug`, `Clone`, `PartialEq`).

**Tests to add in `src/retry.rs`:**
- `classify_http_429_is_transient`.
- `classify_http_500_is_transient`.
- `classify_http_503_is_transient`.
- `classify_http_400_is_permanent`.
- `classify_http_401_is_permanent`.
- `classify_network_error_is_transient`.
- `classify_rate_limited_is_transient`.
- `classify_invalid_request_is_permanent`.
- `classify_aws_throttling_is_transient` — `ProviderError::Aws("ThrottlingException: ...".into())`.
- `classify_aws_timeout_is_transient`.
- `classify_aws_generic_is_permanent`.
- `classify_parse_error_is_permanent`.

**Compile check:** `cargo test --lib` must pass; existing `ProviderError` tests
must still pass.

---

### Step 4 — `TurnEvent::Retrying` + `RetryStatus` TUI state

**Files changed:** `src/types.rs`, `src/tui/mod.rs`, `src/tui/ui.rs`

1. Add `TurnEvent::Retrying { attempt, max_retries, delay_ms, reason }` variant
   to `TurnEvent` in `src/types.rs`.
2. Add `RetryStatus { pub attempt: u32, pub max_retries: u32 }` struct in
   `src/tui/mod.rs` (above `TuiApp`).
3. Add `pub retry_status: Option<RetryStatus>` field to `TuiApp`.
4. Initialise `retry_status: None` in `TuiApp::new` and `TuiApp::headless`.
5. Handle `TurnEvent::Retrying` in `TuiApp::handle_ui_event`:
   - Set `self.retry_status = Some(RetryStatus { attempt, max_retries })`.
6. Clear `self.retry_status = None` in the `TurnEvent::TurnEnd` and
   `TurnEvent::Error` arms of `handle_ui_event`.
7. In `tui/ui.rs`, extend `render_status_bar` (or `format_ctx_segment`) to
   append `│ retrying (N/M)...` to the status bar text when
   `app.retry_status.is_some()`. Add a public helper:
   ```rust
   pub fn format_retry_segment(status: Option<&RetryStatus>) -> String { ... }
   ```
   Returns `""` when `None`, `"retrying (N/M)..."` when `Some`.

**Tests to add:**
- In `src/types.rs`: `turn_event_retrying_is_clonable` — construct the variant,
  clone it, assert fields round-trip.
- In `src/tui/mod.rs`:
  - `handle_ui_event_retrying_sets_retry_status`.
  - `handle_ui_event_turn_end_clears_retry_status`.
  - `handle_ui_event_error_clears_retry_status`.
  - `retry_status_none_by_default`.
- In `src/tui/ui.rs`:
  - `format_retry_segment_none_returns_empty`.
  - `format_retry_segment_some_formats_correctly` — `Some(RetryStatus { attempt: 2, max_retries: 3 })` → `"retrying (2/3)..."`.

**Compile check:** `cargo test --lib` must pass.

---

### Step 5 — `turn_with_retry` core logic

**Files changed:** `src/turn.rs`

1. Add `use tokio::sync::mpsc;` and import `RetryConfig`, `classify_error`,
   `backoff_delay`, `RetryKind` from `crate::retry`.
2. Implement `turn_with_retry`:

```rust
pub async fn turn_with_retry(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
    retry_config: &RetryConfig,
    event_tx: Option<&mpsc::Sender<TurnEvent>>,
) -> Result<(Conversation, Vec<TurnEvent>)> {
    if !retry_config.enabled {
        return turn(conv, provider, tools, middleware).await;
    }

    let mut attempt = 0u32;
    loop {
        match turn(conv.clone(), provider, tools, middleware).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                // Extract ProviderError if present for classification
                let kind = e
                    .downcast_ref::<ProviderError>()
                    .map(classify_error)
                    .unwrap_or(RetryKind::Permanent);

                if kind == RetryKind::Permanent || attempt >= retry_config.max_retries {
                    return Err(e);
                }

                attempt += 1;
                let delay = backoff_delay(retry_config, attempt - 1);

                // Emit Retrying event (best-effort — ignore send errors)
                if let Some(tx) = event_tx {
                    let _ = tx.try_send(TurnEvent::Retrying {
                        attempt,
                        max_retries: retry_config.max_retries,
                        delay_ms: delay.as_millis() as u64,
                        reason: e.to_string(),
                    });
                }

                tokio::time::sleep(delay).await;
            }
        }
    }
}
```

> **Implementation note — `Retry-After` integration:**
> `turn()` surfaces `ProviderError` as an `anyhow::Error` (via `?` in the stream
> loop). `turn_with_retry` uses `anyhow::Error::downcast_ref::<ProviderError>`
> to inspect it. For `ProviderError::Http { retry_after: Some(d), .. }` and
> `ProviderError::RateLimited { retry_after_secs }`, check whether the
> parsed/computed delay exceeds `retry_config.max_delay_ms`; if so, classify as
> `Permanent` (fail immediately with a clear message).

3. Add helper in `classify_error` (or in `turn_with_retry`) to extract an
   explicit delay from the error:
   ```rust
   /// If the error carries an explicit delay, return it; else return `None`.
   pub fn explicit_delay(err: &ProviderError) -> Option<Duration> { ... }
   ```
   Called by `turn_with_retry` to override the backoff delay when present.

**Tests to add in `src/turn.rs`:**

Use a `RetryableMockProvider` that fails N times before succeeding:

```rust
struct RetryableMockProvider {
    fail_times: Arc<Mutex<u32>>,
    error: ProviderError,   // error to return while failing
    success_events: Vec<StreamEvent>,
}
```

- `turn_with_retry_disabled_passes_through` — `RetryConfig { enabled: false, .. }`,
  provider fails once → `turn_with_retry` returns `Err`.
- `turn_with_retry_succeeds_on_second_attempt` — transient error once, then
  success; assert `Ok` result and that events include `TurnEvent::Retrying`.
- `turn_with_retry_exhausts_retries_returns_err` — transient error on every
  call, `max_retries=2`; assert `Err` after 3 total attempts.
- `turn_with_retry_permanent_error_fails_immediately` — `ProviderError::InvalidRequest`
  on first call; assert `Err` with zero `Retrying` events emitted.
- `turn_with_retry_retrying_event_carries_attempt_number` — two transient
  failures then success; assert `Retrying.attempt` is 1 then 2.
- `turn_with_retry_delay_exceeds_max_fails_immediately` — use
  `ProviderError::RateLimited { retry_after_secs }` where `retry_after_secs *
  1000 > max_delay_ms`; assert permanent failure.

For sleep-free tests, override `tokio::time::sleep` using the
`tokio::time::pause()` / `tokio::time::advance()` test helpers (available in
`#[cfg(test)]`). Annotate tests with `#[tokio::test]`.

**Compile check:** `cargo test --lib` must pass; all existing `turn.rs` tests
must still pass.

---

### Step 6 — Call-site wiring (TUI + headless) and `ap.toml.example` update

**Files changed:** `src/tui/mod.rs`, `src/main.rs`, `ap.toml.example`

1. In `TuiApp::handle_submit` (in `tui/mod.rs`), replace the call to `turn()`
   with `turn_with_retry()`.  Pass:
   - `retry_config`: cloned from `conv.config.retry`.
   - `event_tx`: `Some(&tx)` so `Retrying` events flow to the UI channel.
   Import `turn_with_retry` alongside `turn`.

2. In `run_headless` (in `main.rs`), replace the call to `turn()` with
   `turn_with_retry()`.  Pass:
   - `retry_config`: `&conv_to_use.config.retry`.
   - `event_tx`: `None` (headless; retry progress goes to stderr via the event
     router below).

3. In `route_headless_events`, handle `TurnEvent::Retrying`:
   ```rust
   TurnEvent::Retrying { attempt, max_retries, delay_ms, reason } => {
       eprintln!(
           "ap: retrying ({attempt}/{max_retries}) after {delay_ms}ms: {reason}"
       );
   }
   ```

4. Update `ap.toml.example`:
   ```toml
   [retry]
   # enabled = true
   # max_retries = 3
   # base_delay_ms = 2000
   # max_delay_ms = 60000
   ```

**Tests to add:**
- In `src/tui/mod.rs` (unit tests, no real provider needed):
  - `retry_status_cleared_on_turn_end_after_retrying` — push `Retrying` event
    then `TurnEnd`; assert `retry_status` is `None`.
  - `retry_status_cleared_on_error_after_retrying` — push `Retrying` then
    `Error`; assert `retry_status` is `None`.
- In `src/main.rs`:
  - `route_headless_events_retrying_returns_0` — `Retrying` event alone produces
    exit code 0 (not an error).

**Compile check:** `cargo build` and `cargo test` must pass with no warnings
under the existing `clippy` lint set.

---

## Acceptance Criteria

All of the following must hold before marking this item complete.

1. **Config parses correctly.**
   `AppConfig::load_with_paths` with a TOML file containing a full `[retry]`
   table populates all four `RetryConfig` fields. A file with no `[retry]`
   table yields the defaults (`enabled=true`, `max_retries=3`,
   `base_delay_ms=2000`, `max_delay_ms=60000`). Partial overlay preserves
   unset defaults.

2. **`backoff_delay` is correct.**
   Attempt 0 → `base_delay_ms`, attempt 1 → `2×base_delay_ms`, attempt 2 →
   `4×base_delay_ms`. Capped at `max_delay_ms`. No panic on extreme attempt
   numbers.

3. **`parse_retry_after` parses integer seconds.**
   `"30"` → `Duration::from_secs(30)`. Non-integer non-date → `None`.

4. **`classify_error` is correct.**
   `Http { status: 429 }` and `Http { status: 503 }` → `Transient`.
   `Http { status: 400 }` and `Http { status: 401 }` → `Permanent`.
   `Network(_)` → `Transient`. `InvalidRequest(_)` → `Permanent`.
   `Aws("ThrottlingException")` → `Transient`.

5. **`turn_with_retry` retries exactly N times on transient errors.**
   With `max_retries=2` and a provider that always fails with a transient error,
   `turn_with_retry` makes exactly 3 total attempts (1 initial + 2 retries)
   before returning `Err`.

6. **`turn_with_retry` does not retry permanent errors.**
   A `ProviderError::InvalidRequest` on the first call returns `Err`
   immediately, with `attempt_count == 1`.

7. **`turn_with_retry` emits `TurnEvent::Retrying` with correct fields.**
   On the first retry, `attempt=1`, `max_retries` matches config, `delay_ms`
   matches `backoff_delay(config, 0)`.

8. **Retry-After delay exceeding `max_delay_ms` fails immediately.**
   `ProviderError::RateLimited { retry_after_secs: 7200 }` with
   `max_delay_ms=60000` → permanent failure, no retry, clear error message.

9. **TUI status bar shows retry progress.**
   `TuiApp::handle_ui_event(TurnEvent::Retrying { attempt: 2, max_retries: 3, .. })`
   sets `app.retry_status = Some(RetryStatus { attempt: 2, max_retries: 3 })`.
   `format_retry_segment(Some(&status))` returns `"retrying (2/3)..."`.

10. **Retry status clears on completion.**
    After `TurnEnd` or `Error`, `app.retry_status` is `None`.

11. **`cargo build` is clean.**
    `cargo build 2>&1 | grep -E '^error'` produces no output. All existing tests
    pass (`cargo test`).

12. **Functional invariants preserved.**
    `turn_with_retry` with `RetryConfig { enabled: false, .. }` behaves
    identically to calling `turn()` directly.

---

## Implementation Notes

- **No new crates.** All retry logic uses `std` and the already-present
  `tokio`, `anyhow`, `thiserror`. Do not add `backoff`, `retry`, or any HTTP
  date-parsing crate.
- **Clippy compliance.** The project denies `unwrap_used`, `expect_used`,
  `panic`. Use `?`, `unwrap_or`, `unwrap_or_else`, or explicit `match` in all
  new code.
- **Test sleep duration.** In unit tests use `tokio::time::pause()` before
  calling `turn_with_retry` so the `tokio::time::sleep` calls complete
  instantly. Reset with `tokio::time::resume()` after the test or rely on test
  isolation.
- **`ProviderError` is non-exhaustive in tests.** When adding new variants,
  update the wildcard arm `_ => {}` in `classify_error` to remain exhaustive.
- **`anyhow::Error` downcast.** `turn()` wraps `ProviderError` values via the
  `?` operator in the stream loop, which calls `anyhow::Error::from`. Use
  `err.downcast_ref::<ProviderError>()` in `turn_with_retry`; this correctly
  recovers the original variant.
- **Order of precedence for delay:** explicit `Retry-After` header >
  computed backoff. When an explicit delay exceeds `max_delay_ms`, fail
  immediately with the message:
  `"provider requested retry delay of Xs which exceeds max_delay_ms (Yms)"`.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project
builds clean.
