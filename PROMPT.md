# PROMPT.md — Retry with Exponential Backoff

## Vision

`ap` calls AWS Bedrock over a streaming API. Transient failures — rate limits,
server hiccups, dropped SSE streams — should not surface as errors to the user.
Instead, `turn()` silently retries with exponential backoff, emits a
`TurnEvent::Retrying` so the TUI can show a live `retrying (2/3)...` indicator
in the status bar, and only fails permanently when the attempt budget is
exhausted or the error is non-retryable (auth failures, invalid requests).

Retry behaviour is controlled by a `[retry]` TOML section that follows the same
two-file layered merge pattern as every other config section (`~/.ap/config.toml`
global, `./ap.toml` project-level, project wins).

---

## Architecture constraints (non-negotiable)

- `turn()` signature is **unchanged** — callers in `main.rs` and `tui/mod.rs`
  must compile without modification.
- All retry logic lives in a new `src/retry.rs` module — not inline in
  `turn_loop`.
- `Conversation` remains immutable — retry only restarts the *provider stream*
  for the current LLM call, not the whole agent turn.
- No `tokio::time::sleep` inside `provider/` — sleep belongs to the retry layer
  in `turn.rs`.
- `tokio` is already in `Cargo.toml` with `features = ["full"]`; no new
  dependencies are needed.
- All new public types carry `#[derive(Debug, Clone)]` and pass serde
  roundtrips where serialised.
- `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` is
  required on every `#[cfg(test)]` block.

---

## New / changed files

| File | Change |
|---|---|
| `src/config.rs` | Add `RetryConfig` struct + field on `AppConfig` + overlay |
| `src/provider/mod.rs` | Extend `ProviderError`; add `is_retryable()` + `retry_after_hint()` |
| `src/provider/bedrock.rs` | Map AWS SDK errors to typed `ProviderError` variants |
| `src/types.rs` | Add `TurnEvent::Retrying { attempt, max_attempts }` variant |
| `src/retry.rs` | **New** — pure retry engine |
| `src/turn.rs` | Wire `RetryPolicy` into `turn_loop` stream call |
| `src/tui/mod.rs` | Add `retry_status` field + handle `TurnEvent::Retrying` |
| `src/tui/ui.rs` | Render `retrying (N/M)...` in status bar |
| `src/lib.rs` | `pub mod retry;` |

---

## Detailed type signatures

### `src/config.rs`

```rust
/// Retry-with-backoff configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetryConfig {
    /// Master switch — set to `false` to disable all retries.
    pub enabled: bool,
    /// Maximum number of retry attempts (not counting the first attempt).
    pub max_retries: u32,
    /// Base delay in milliseconds for the first retry.
    pub base_delay_ms: u64,
    /// Hard ceiling on computed delay in milliseconds.
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            base_delay_ms: 2_000,
            max_delay_ms: 60_000,
        }
    }
}
```

Add `pub retry: RetryConfig` to `AppConfig` with `#[serde(default)]`.

Wire into `overlay_from_table` under a `[retry]` key, overlaying only the keys
present in the TOML table (same pattern as `[context]`).

### `src/provider/mod.rs`

Extend `ProviderError`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    // existing variants unchanged …
    #[error("AWS error: {0}")]
    Aws(String),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    // NEW ──────────────────────────────────────────────────────────────────
    /// HTTP 429 / ThrottlingException — retryable.
    /// `retry_after_secs` is populated when the provider includes a hint.
    #[error("rate limited: {message}")]
    RateLimited { message: String, retry_after_secs: Option<u64> },

    /// HTTP 5xx / server-side error — retryable.
    #[error("server error: {0}")]
    ServerError(String),
}
```

Add two methods on `ProviderError`:

```rust
impl ProviderError {
    /// Returns `true` if this error class is safe to retry.
    ///
    /// Retryable:  `RateLimited`, `ServerError`
    /// Not retryable: `Aws` (catch-all), `ParseError`, `Serialization`
    ///
    /// Note: `Aws(String)` is non-retryable by default. `BedrockProvider`
    /// maps specific AWS SDK errors to `RateLimited` / `ServerError` before
    /// they reach this layer.
    pub fn is_retryable(&self) -> bool { … }

    /// Returns the provider-supplied retry hint, if any.
    ///
    /// Only `RateLimited { retry_after_secs: Some(n) }` yields `Some`.
    pub fn retry_after_hint(&self) -> Option<std::time::Duration> { … }
}
```

### `src/provider/bedrock.rs`

Inside `stream_completion`, after the `client.invoke_model_with_response_stream()` call,
map specific AWS SDK error strings to typed variants before wrapping in
`ProviderError::Aws`:

```rust
// Heuristic error classification — check error string for known patterns.
fn classify_aws_error(raw: &str) -> ProviderError {
    let lower = raw.to_lowercase();
    if lower.contains("throttlingexception")
        || lower.contains("toomanyrequestsexception")
        || lower.contains("429")
    {
        // Attempt to parse "Retry-After: N" from the message (best-effort).
        let retry_after_secs = parse_retry_after(raw);
        ProviderError::RateLimited { message: raw.to_string(), retry_after_secs }
    } else if lower.contains("serviceunavailableexception")
        || lower.contains("internalserverexception")
        || lower.contains("internalfailure")
        || lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        ProviderError::ServerError(raw.to_string())
    } else {
        ProviderError::Aws(raw.to_string())
    }
}

/// Extract a `Retry-After` value (integer seconds) from an error string.
/// Returns `None` when no hint can be found.
fn parse_retry_after(s: &str) -> Option<u64> { … }
```

Replace `.map_err(|e| ProviderError::Aws(e.to_string()))` on the `send()` call
with `.map_err(|e| classify_aws_error(&e.to_string()))`.

### `src/types.rs`

Add one variant to `TurnEvent`:

```rust
/// A retryable provider error occurred; the pipeline is sleeping before
/// the next attempt.
Retrying {
    /// 1-based attempt number that just failed (so first retry → attempt = 1).
    attempt: u32,
    /// Maximum number of retries allowed.
    max_attempts: u32,
},
```

### `src/retry.rs` (new file)

```rust
// src/retry.rs — Pure, synchronous retry-decision engine.
//
// No I/O here. The caller (turn.rs) owns the sleep and the channel send.

use std::time::Duration;
use crate::config::RetryConfig;
use crate::provider::ProviderError;

/// Decision returned by `should_retry`.
#[derive(Debug)]
pub enum RetryDecision {
    /// Sleep for this duration, then retry.
    Retry(Duration),
    /// Give up — surface this error to the caller.
    GiveUp(ProviderError),
}

/// Compute the exponential backoff delay for `attempt` (0-based: 0 = first retry).
///
/// Formula: `min(base_delay_ms * 2^attempt, max_delay_ms)` milliseconds.
/// Saturating arithmetic prevents overflow on large attempt counts.
pub fn compute_delay(attempt: u32, config: &RetryConfig) -> Duration {
    let multiplier = 1u64.saturating_shl(attempt);          // 2^attempt
    let ms = config.base_delay_ms.saturating_mul(multiplier);
    let capped = ms.min(config.max_delay_ms);
    Duration::from_millis(capped)
}

/// Decide whether to retry after `error` on attempt number `attempt`
/// (0-based: 0 = deciding after the first failure).
///
/// Rules (in priority order):
/// 1. If `config.enabled` is `false` → `GiveUp`.
/// 2. If `attempt >= config.max_retries` → `GiveUp`.
/// 3. If `!error.is_retryable()` → `GiveUp`.
/// 4. If `error.retry_after_hint()` is `Some(d)` and `d > max_delay_ms` ms
///    → `GiveUp` (quota reset too far in the future).
/// 5. Use `error.retry_after_hint()` if `Some`, else `compute_delay`.
/// 6. → `Retry(delay)`.
pub fn should_retry(
    error: ProviderError,
    attempt: u32,
    config: &RetryConfig,
) -> RetryDecision { … }
```

### `src/turn.rs`

Inside `turn_loop`, the inner `while let Some(event) = stream.next().await` block
currently returns `Err` on the first error. Replace that block with a retry-aware
wrapper:

```rust
// Conceptual structure (exact placement inside turn_loop):
let mut stream_attempt: u32 = 0;
loop {
    // … rebuild stream from provider …
    match drive_stream(&mut stream, &mut assistant_text, &mut pending_tools, &mut all_events).await {
        Ok(()) => break,
        Err(provider_err) => {
            match retry::should_retry(provider_err, stream_attempt, &conv.config.retry) {
                retry::RetryDecision::Retry(delay) => {
                    all_events.push(TurnEvent::Retrying {
                        attempt: stream_attempt + 1,
                        max_attempts: conv.config.retry.max_retries,
                    });
                    tokio::time::sleep(delay).await;
                    stream_attempt += 1;
                    // Reset accumulators before re-streaming
                    assistant_text.clear();
                    pending_tools.clear();
                    // strip the last Retrying event from all_events so it's
                    // not double-counted (it was already sent to the TUI channel
                    // separately via the return path in the TUI task) — ONLY
                    // needed when events are buffered; with the channel pattern
                    // in tui/mod.rs events are sent post-turn so this is fine.
                    continue;
                }
                retry::RetryDecision::GiveUp(e) => {
                    let msg = e.to_string();
                    all_events.push(TurnEvent::Error(msg.clone()));
                    return Err(anyhow::anyhow!(msg));
                }
            }
        }
    }
}
```

Extract the stream-consumption logic into a private helper:

```rust
/// Drive a single provider stream to completion, appending events.
///
/// Returns `Ok(())` on clean `TurnEnd`, or `Err(ProviderError)` on the first
/// error event.  Callers are responsible for retry logic.
async fn drive_stream(
    stream: &mut BoxStream<'_, Result<StreamEvent, ProviderError>>,
    assistant_text: &mut String,
    pending_tools: &mut Vec<PendingTool>,
    all_events: &mut Vec<TurnEvent>,
) -> Result<(), ProviderError> { … }
```

### `src/tui/mod.rs`

Add field to `TuiApp`:

```rust
/// Current retry status shown in the status bar.
/// `None` = not retrying. `Some((attempt, max))` = attempt N of max.
pub retry_status: Option<(u32, u32)>,
```

Initialise to `None` in `new()` and `headless()`.

Handle new event in `handle_ui_event`:

```rust
TurnEvent::Retrying { attempt, max_attempts } => {
    self.retry_status = Some((attempt, max_attempts));
}
```

Clear `retry_status` on `TurnEnd` and `Error`:

```rust
TurnEvent::TurnEnd => {
    self.retry_status = None;
    // … existing logic …
}
TurnEvent::Error(_) => {
    self.retry_status = None;
    // … existing logic …
}
```

### `src/tui/ui.rs`

In `render_status_bar`, append the retry segment when `app.retry_status` is
`Some`:

```rust
let retry_segment = app.retry_status
    .map(|(attempt, max)| format!(" │ retrying ({attempt}/{max})..."))
    .unwrap_or_default();
let text = format!(
    " ap │ {} │ {} │ Msgs: {} │ Tokens: ↑{:.1}k ↓{:.1}k │ Cost: ${:.4} │ {}{}",
    app.model_name, mode_label, app.conversation_messages,
    input_k, output_k, cost, ctx_segment, retry_segment,
);
```

---

## Ordered implementation steps

Each step must leave the project in a **clean compile + all existing tests
passing** state before the next step begins.

### Step 1 — `RetryConfig` in `config.rs`

1. Define `RetryConfig` with defaults as above.
2. Add `pub retry: RetryConfig` to `AppConfig` (`#[serde(default)]`).
3. Wire into `overlay_from_table` under `"retry"` key.
4. Write tests:
   - `retry_config_defaults()` — all four fields at expected values.
   - `retry_config_toml_full()` — parse all four fields from TOML.
   - `retry_config_missing_keys_preserve_defaults()` — partial TOML leaves unset
     fields at default.
   - `retry_config_disabled_from_toml()` — `enabled = false` parses correctly.
   - `retry_config_project_overrides_global()` — project `max_retries` wins.

**Compile gate:** `cargo test -q` passes (204 + new tests).

---

### Step 2 — Extend `ProviderError` + classify Bedrock errors

1. Add `RateLimited` and `ServerError` variants to `ProviderError`.
2. Implement `is_retryable()` and `retry_after_hint()`.
3. Add `classify_aws_error()` and `parse_retry_after()` in `bedrock.rs`.
4. Replace `.map_err(|e| ProviderError::Aws(e.to_string()))` on the AWS `send()`
   call with `.map_err(|e| classify_aws_error(&e.to_string()))`.
5. Write tests in `provider/mod.rs`:
   - `rate_limited_is_retryable()`.
   - `server_error_is_retryable()`.
   - `aws_error_not_retryable()`.
   - `parse_error_not_retryable()`.
   - `rate_limited_with_hint_returns_duration()`.
   - `rate_limited_without_hint_returns_none()`.
6. Write tests in `provider/bedrock.rs`:
   - `classify_throttling_exception()` → `RateLimited`.
   - `classify_service_unavailable()` → `ServerError`.
   - `classify_unknown_aws_error()` → `Aws`.
   - `parse_retry_after_from_string()` — extracts seconds when present.
   - `parse_retry_after_absent()` → `None`.

**Compile gate:** all existing + new tests pass.

---

### Step 3 — `TurnEvent::Retrying` variant

1. Add `Retrying { attempt: u32, max_attempts: u32 }` to `TurnEvent` in
   `types.rs`.
2. Ensure existing `match` sites remain exhaustive (add `TurnEvent::Retrying`
   arms in `tui/mod.rs::handle_ui_event`, `main.rs::route_headless_events`).
   Both are no-ops for now.
3. Write tests in `types.rs`:
   - `turn_event_retrying_clonable()` — clone and check fields.
   - `turn_event_retrying_debug()` — `format!("{:?}", ...)` contains `"Retrying"`.

**Compile gate:** all tests pass, no new warnings.

---

### Step 4 — `src/retry.rs` pure engine

1. Create `src/retry.rs` with `RetryDecision`, `compute_delay`, `should_retry`.
2. Add `pub mod retry;` to `src/lib.rs`.
3. Write tests (all in `retry.rs`):
   - `compute_delay_attempt_0()` — equals `base_delay_ms`.
   - `compute_delay_attempt_1()` — equals `2 * base_delay_ms`.
   - `compute_delay_attempt_2()` — equals `4 * base_delay_ms`.
   - `compute_delay_capped_at_max()` — large attempt number clamps to
     `max_delay_ms`.
   - `should_retry_disabled_gives_up()` — `enabled = false` → `GiveUp`.
   - `should_retry_max_retries_exceeded_gives_up()` — `attempt >= max_retries`
     → `GiveUp`.
   - `should_retry_non_retryable_gives_up()` — `Aws` error → `GiveUp`.
   - `should_retry_rate_limited_retries()` — `RateLimited` with no hint uses
     computed delay.
   - `should_retry_uses_hint_when_present()` — `retry_after_secs` in range uses
     that duration.
   - `should_retry_hint_exceeds_max_delay_gives_up()` — `retry_after_secs`
     mapped to ms > `max_delay_ms` → `GiveUp`.
   - `should_retry_server_error_retries()`.

**Compile gate:** all tests pass.

---

### Step 5 — Wire retry into `turn_loop`

1. Extract `drive_stream()` private async fn from the existing stream loop in
   `turn_loop`.
2. Wrap the call with a `loop` that calls `retry::should_retry` on `Err`.
3. Emit `TurnEvent::Retrying` before sleeping.
4. Reset `assistant_text` and `pending_tools` before each retry so partial
   streamed content doesn't corrupt the assistant message.
5. Write tests in `turn.rs`:
   - `turn_retries_on_rate_limit_then_succeeds()` — provider fails with
     `RateLimited` once then returns a normal text stream; turn succeeds,
     `TurnEvent::Retrying { attempt: 1, max_attempts: 3 }` is in events, no
     `TurnEvent::Error`.
   - `turn_gives_up_after_max_retries()` — provider always returns
     `RateLimited`; after `max_retries` attempts, `turn()` returns `Err` and
     events contain `TurnEvent::Error`.
   - `turn_no_retry_on_non_retryable_error()` — `Aws` error → immediate `Err`,
     no `Retrying` event.
   - `turn_retry_disabled_does_not_retry()` — `RetryConfig { enabled: false }`
     → immediate `Err` on first failure, no `Retrying` event.
   - `turn_retrying_event_carries_correct_counts()` — second failure → attempt=2.

   For these tests, use the existing `MockProvider` pattern (`VecDeque<Vec<StreamEvent>>`)
   extended to support `Err` entries:

   ```rust
   struct FallibleProvider {
       // Each element: Ok(events) or Err(error)
       scripts: Arc<Mutex<VecDeque<Result<Vec<StreamEvent>, ProviderError>>>>,
   }
   ```

   Use `tokio::time::pause()` + `tokio::time::advance()` (available with
   `tokio::test` and `features = ["full"]`) to avoid real wall-clock delays in
   tests.

**Compile gate:** all tests pass, including the new retry tests (which rely on
frozen time).

---

### Step 6 — TUI integration

1. Add `retry_status: Option<(u32, u32)>` to `TuiApp`.
2. Initialise to `None` in `new()` and `headless()`.
3. Handle `TurnEvent::Retrying` in `handle_ui_event`.
4. Clear `retry_status` on `TurnEvent::TurnEnd` and `TurnEvent::Error`.
5. Render in `render_status_bar` (see signature above).
6. Handle `TurnEvent::Retrying` in `main.rs::route_headless_events` (print
   `"ap: retrying ({attempt}/{max_attempts})...\n"` to stderr; return existing
   exit code unchanged).
7. Write tests:
   - `handle_ui_event_retrying_sets_status()` — after `Retrying { 2, 3 }`,
     `app.retry_status == Some((2, 3))`.
   - `handle_ui_event_turn_end_clears_retry_status()`.
   - `handle_ui_event_error_clears_retry_status()`.
   - `status_bar_shows_retry_segment()` — call `render_status_bar` logic or use
     `format_ctx_segment` analogue; assert the formatted string contains
     `"retrying (2/3)..."`.
   - `status_bar_no_retry_segment_when_none()` — `retry_status = None` → string
     does not contain `"retrying"`.

**Compile gate:** `cargo test -q` passes; `cargo clippy -- -D warnings` is clean.

---

## Acceptance criteria

All of the following must be true before the loop is considered complete:

1. **Config parsing** — `RetryConfig` round-trips through TOML; `AppConfig::load()`
   with `[retry] max_retries = 5` returns a config with `retry.max_retries == 5`.

2. **Error classification** — `ProviderError::RateLimited` and `ServerError`
   return `true` from `is_retryable()`; all other variants return `false`.

3. **Delay formula** — `compute_delay(0, &default_config())` returns 2 s;
   `compute_delay(1, …)` returns 4 s; a large attempt is capped at 60 s.

4. **Retry-After respected** — when `retry_after_secs = Some(10)` and
   `max_delay_ms = 60_000`, `should_retry` returns `Retry(10 s)`.

5. **Retry-After too large** — when `retry_after_secs = Some(18_001)` (> 18 000 s
   = 300 min) with `max_delay_ms = 60_000`, `should_retry` returns `GiveUp`.

6. **Turn retries transparently** — a provider that fails once with `RateLimited`
   then succeeds produces a final `(Conversation, Vec<TurnEvent>)` where
   events contain exactly one `TurnEvent::Retrying` and one `TurnEvent::TurnEnd`,
   and `turn()` returns `Ok`.

7. **Non-retryable errors fail immediately** — a `ProviderError::Aws` on the
   first stream attempt causes `turn()` to return `Err` with zero `Retrying`
   events.

8. **Max retries exhaustion** — a provider that always returns `RateLimited`
   causes `turn()` to return `Err` after emitting `max_retries` `Retrying`
   events.

9. **TUI retry status** — `TuiApp::handle_ui_event(TurnEvent::Retrying { attempt: 2, max_attempts: 3 })`
   sets `app.retry_status = Some((2, 3))`; subsequent `TurnEnd` clears it to
   `None`.

10. **Status bar rendering** — when `retry_status = Some((1, 3))`, the rendered
    status bar string contains `"retrying (1/3)..."`.

11. **Clean build** — `cargo build --release` succeeds with zero warnings.

12. **All tests pass** — `cargo test -q` reports 0 failures across all test
    targets.

---

## Anti-patterns to avoid

- **Do not** add `async` to `should_retry` or `compute_delay` — they are pure
  synchronous functions.
- **Do not** change the `Provider` trait — retry wraps the *caller*, not the
  implementor.
- **Do not** sleep inside `provider/bedrock.rs` — sleeping belongs to `turn.rs`.
- **Do not** add a `Mutex` or shared state for retry tracking — `stream_attempt`
  is a local `u32` in `turn_loop`.
- **Do not** retry tool-execution errors — only *provider stream* errors are
  retried; tool errors are passed through unchanged.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project
builds clean.
