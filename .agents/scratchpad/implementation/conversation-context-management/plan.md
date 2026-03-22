# Implementation Plan — Conversation Context Management

## Test Strategy

### Unit Tests

#### `src/context.rs` — Pure Functions (Step 1, 8 tests)

| Test | Input | Expected |
|------|-------|----------|
| `estimate_tokens_empty` | `&[]` | `0` |
| `estimate_tokens_text_message` | Single `Text { text: "hello world" }` (11 chars) | `11/4 = 2`, min 1 → `2` |
| `estimate_tokens_tool_use` | `ToolUse { name: "bash", input: json!("ls") }` | `(4 + 4) / 4 = 2` |
| `estimate_tokens_tool_result` | `ToolResult { content: "output\n", .. }` | `7/4 = 1` |
| `find_summary_split_too_short` | 3 messages, keep_recent=5 | `None` |
| `find_summary_split_finds_user` | 10 messages, keep_recent=5, msg[5] is User | `Some(5)` |
| `find_summary_split_skips_to_user` | 10 messages, keep_recent=5, msg[5] is Assistant, msg[6] is User | `Some(6)` |
| `find_summary_split_no_user_in_tail` | 10 messages, keep_recent=5, tail has only Assistant | `None` |

#### `src/config.rs` — ContextConfig (Step 2, 5 tests)

| Test | Scenario | Expected |
|------|----------|----------|
| `context_config_defaults` | `ContextConfig::default()` | `limit=None, keep_recent=20, threshold=0.80` |
| `context_config_toml_limit` | `[context]\nlimit = 100000` | `limit=Some(100000)`, others default |
| `context_config_toml_full` | all three keys set | all three parsed correctly |
| `context_config_missing_keys_preserve_defaults` | partial table (only `limit`) | unset keys stay at defaults |
| `context_config_no_auto_summarize_when_limit_none` | `AppConfig::default()` | `config.context.limit == None` |

#### `src/tui/mod.rs` + `src/tui/ui.rs` — TUI Events & Status Bar (Step 4, 6 tests)

| Test | Scenario | Expected |
|------|----------|----------|
| `turn_event_context_summarized_clonable` | Clone the new variant | Clone compiles and roundtrips |
| `handle_ui_event_context_summarized_appends_notice` | Send `ContextSummarized` event | `chat_history` grows by 1 |
| `handle_ui_event_usage_updates_last_input_tokens` | Send `Usage { input_tokens: 5000 }` | `last_input_tokens == 5000` |
| `handle_ui_event_usage_still_accumulates_totals` | Send two `Usage` events | `total_input_tokens` accumulates both |
| `status_bar_ctx_display_no_limit` | `last_input_tokens=45200, context_limit=None` | Status contains `"ctx: 45.2k"` |
| `status_bar_ctx_display_with_limit` | `last_input_tokens=45200, context_limit=Some(200000)` | Status contains `"ctx: 45.2k/200k (23%)"` |

#### `src/context.rs` — Async Functions (Step 5, 6 tests)

| Test | Scenario | Expected |
|------|----------|----------|
| `summarise_messages_collects_stream` | MockProvider returns `TextDelta("foo")`, `TextDelta("bar")`, TurnEnd | Result is `Ok("foobar")` |
| `summarise_messages_provider_error_returns_err` | ErrorProvider returns error stream | Result is `Err(...)` |
| `maybe_compress_context_no_op_under_threshold` | Estimated tokens < `limit * threshold` | Returns `(conv, None)` |
| `maybe_compress_context_compresses_when_over_threshold` | Estimated tokens >= threshold | `messages_after < messages_before`, event is `Some` |
| `maybe_compress_context_new_messages_start_with_user` | Compression fires | `result.conv.messages[0].role == Role::User` |
| `maybe_compress_context_cannot_split_returns_unchanged` | Too few messages to split | Returns `(conv, None)` |

#### `src/tui/mod.rs` — TuiApp Constructor (Step 7, 1 test)

| Test | Scenario | Expected |
|------|----------|----------|
| `tuiapp_new_stores_context_limit` | `TuiApp::headless_with_limit(Some(50_000))` | `app.context_limit == Some(50_000)` |

**Total: 26 tests (8 + 5 + 6 + 6 + 1)**

### Integration Notes

- **No integration test file** — all tests are unit-level in-module `#[cfg(test)]` blocks
- **MockProvider / ErrorProvider** — copy structs from `src/turn.rs` tests into `src/context.rs` `#[cfg(test)]` block
- **Existing tests** — all 24 `headless()` call sites are unaffected; `headless()` signature unchanged

### E2E Test Scenario

**Goal:** Verify that `--context-limit` compresses context and surfaces it in the TUI.

**Setup:** Start `ap` in headless mode with a low limit to force compression.

**Steps:**
1. Create a test conversation with many messages: `ap --context-limit 500 -p "hello"`
2. Observe: should complete without panic, binary runs
3. Adversarial path: `ap --context-limit 1 -p "hello"` — limit so low that even 1 message triggers compression; must not crash (should gracefully handle cannot-split case)
4. Build check: `cargo build --release` must emit zero warnings

**Harness:** Real CLI (`cargo run -- --context-limit ...`), not Playwright.

---

## Implementation Plan

### Step 1 — `src/context.rs`: Pure token estimation + split logic

**Files to create/modify:**
- `ap/src/context.rs` (new)
- `ap/src/lib.rs` (add `pub mod context;`)

**Implementation:**
- `estimate_message_tokens(msg: &Message) -> u32` — chars/4 per content variant, min 1
- `estimate_tokens(messages: &[Message]) -> u32` — sum over slice
- `find_summary_split(messages: &[Message], keep_recent: usize) -> Option<usize>` — alternating-turn-safe split

**Tests that must pass after this step:** 8 unit tests in `src/context.rs`

**Connects to:** Nothing yet — pure functions, no deps on other new code

**Demo:** `cargo test context::tests` passes 8 tests; `cargo build` clean

---

### Step 2 — `ContextConfig` in `AppConfig`

**Files to modify:**
- `ap/src/config.rs`

**Implementation:**
- Add `ContextConfig` struct with `#[serde(default)]`, 3 fields, `Default` impl
- Add `pub context: ContextConfig` to `AppConfig` with `#[serde(default)]`
- Extend `overlay_from_table` to handle `[context]` table key

**Tests that must pass after this step:** 5 new tests in `src/config.rs` + all existing config tests

**Connects to:** Step 1 (module exists but not called yet)

**Demo:** `cargo test config::tests` passes 5 new tests; session files without `"context"` key still deserialize

---

### Step 3 — `--context-limit` CLI flag

**Files to modify:**
- `ap/src/main.rs`

**Implementation:**
- Add `context_limit: Option<u32>` to `Args` struct
- After `AppConfig::load()`, override `config.context.limit` when CLI arg is present
- Store limit as local variable for future use (no `maybe_compress_context` call yet)

**Tests that must pass:** All existing tests; `cargo build` clean

**Connects to:** Step 2 (reads `config.context.limit`)

**Demo:** `ap --context-limit 50000 -p "hello"` compiles and runs (compression not wired yet, but flag accepted)

---

### Step 4 — `TurnEvent::ContextSummarized` + TUI fields + status bar

**Files to modify:**
- `ap/src/types.rs` — add `ContextSummarized` variant
- `ap/src/tui/mod.rs` — add `last_input_tokens` and `context_limit` fields; handle new event
- `ap/src/tui/ui.rs` — extend `render_status_bar` with ctx display
- `ap/src/main.rs` — add `TurnEvent::ContextSummarized` arm in `route_headless_events` (compile requirement)

**Implementation:**
- New `TurnEvent::ContextSummarized { messages_before, messages_after, tokens_before, tokens_after }` — must be `Clone`
- `TuiApp::last_input_tokens: u32` (default 0)
- `TuiApp::context_limit: Option<u32>` (default None; wired in Step 7)
- `handle_ui_event` additions:
  - `Usage { input_tokens, .. }` → `self.last_input_tokens = input_tokens`
  - `ContextSummarized { .. }` → push chat notice + set `self.last_input_tokens = tokens_after`
- `render_status_bar` ctx segment: `│ ctx: XX.Xk` always; `/YYYk (ZZ%)` when limit is Some
- `headless()` constructor sets both new fields to defaults
- `route_headless_events` match arm for `ContextSummarized` → `eprintln!(...)` (prevents non-exhaustive error)

**Tests that must pass:** 6 new tests in `src/tui/mod.rs` and `src/tui/ui.rs`; all existing tests

**Connects to:** Step 3 (event variant referenced in main.rs route)

**Demo:** Status bar renders `ctx: 0.0k` (with zero tokens); event system compiles

---

### Step 5 — `src/context.rs`: Async summarisation + `maybe_compress_context`

**Files to modify:**
- `ap/src/context.rs` — add `summarise_messages` and `maybe_compress_context`
- Ensure `Cargo.toml` has `futures` (already present per Explorer)

**Implementation:**
- `summarise_messages` — builds summary prompt, calls `provider.stream_completion`, drains stream collecting `TextDelta`, returns accumulated string
- `maybe_compress_context` — orchestrates: estimate → threshold check → find_split → summarise → build `new_messages` with User summary wrapper + recent → `conv.with_messages(new_messages)` → return event
- `Conversation::with_messages(Vec<Message>) -> Self` consuming builder — add to `src/types.rs`
- `MockProvider` and `ErrorProvider` in `#[cfg(test)]` block in `context.rs`

**Tests that must pass:** 6 new async tests; all existing tests

**Connects to:** Steps 1 (calls pure fns), 4 (returns `TurnEvent::ContextSummarized`)

**Demo:** `cargo test context::tests` passes all 14 tests (8 + 6); compression logic verified in isolation

---

### Step 6 — Wire `maybe_compress_context` in headless path

**Files to modify:**
- `ap/src/main.rs` — `run_headless` function

**Implementation:**
- Import `ap::context::maybe_compress_context`
- Clone `conv_with_msg` as `fallback` before the `maybe_compress_context` call
- Wrap call in `if let Some(limit) = config.context.limit` guard
- On `Ok((c, evt))`: use `c`, log `evt` to stderr
- On `Err(e)`: log warning, use `fallback`
- (When limit is None: proceed directly to `turn()` as before)

**Tests that must pass:** All existing tests (behavior unchanged when limit is None)

**Connects to:** Step 5 (calls `maybe_compress_context`)

**Demo:** `ap --context-limit 500 -p "hello"` runs; with low limit and long history, compression fires and logs to stderr

---

### Step 7 — Wire TUI path + `headless_with_limit` constructor

**Files to modify:**
- `ap/src/tui/mod.rs` — `TuiApp::new` signature, spawned task in `handle_submit`, `headless_with_limit` constructor
- `ap/src/main.rs` — `run_tui` call to `TuiApp::new`

**Implementation:**
- Add `context_limit: Option<u32>` parameter to `TuiApp::new`; store in `self.context_limit`
- `run_tui` in `main.rs` passes `config.context.limit` to `TuiApp::new`
- In spawned task (`handle_submit`):
  - Clone `conv_with_msg` is already owned; clone before `maybe_compress_context` for fallback
  - Guard with `if let Some(limit) = self.context_limit`
  - On `Ok((c, Some(evt)))` → `tx.send(evt).await.ok()`
  - On `Err(e)` → `tx.send(TurnEvent::Error(...)).await.ok(); return`
  - Capture `context_limit`, `keep_recent`, `threshold` as `Copy` scalars into closure
- Add `pub fn headless_with_limit(context_limit: Option<u32>) -> Self` — identical to `headless()` but stores the limit
- `headless()` unchanged (delegates to `headless_with_limit(None)` internally, or kept as-is)

**Tests that must pass:** 1 new test (`tuiapp_new_stores_context_limit`); all 24 existing `headless()` call sites pass unchanged

**Connects to:** Step 6 (full end-to-end pipeline complete)

**Demo:** Full TUI session with `--context-limit` set shows `ctx: XX.Xk/YYYk (ZZ%)` in status bar; compression fires and pushes notice to chat history

---

## Success Criteria Summary

| Check | Verified at |
|-------|-------------|
| `cargo build --release` — zero errors, zero warnings | Each step |
| `cargo test` — zero failures | Each step |
| `cargo clippy -- -D warnings` — zero | Step 7 (full pass) |
| 26 new tests present and passing | Step 7 |
| `src/context.rs` exports all 5 public items | Step 5 |
| `AppConfig.context: ContextConfig` with correct defaults | Step 2 |
| `TurnEvent::ContextSummarized` exists and is `Clone` | Step 4 |
| `TuiApp` has `last_input_tokens` and `context_limit` | Step 4 |
| Status bar renders `ctx: XX.Xk` always + `%` when limit | Step 4 |
| `ap --context-limit 50000 -p "hello"` runs without panic | Step 6 |
| No-split no-op (too few messages) | Step 5 |
| First message after compression is `Role::User` | Step 5 |
