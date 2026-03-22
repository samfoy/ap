# PROMPT.md — Conversation Context Management

## Vision

Long coding sessions accumulate hundreds of messages and tens of thousands of
tokens. When the context window fills, the provider returns an error and the
session dies.  This feature prevents that by:

1. **Estimating** token usage before every call.
2. **Auto-summarizing** old messages when the estimate crosses a configurable
   threshold, keeping a rolling recent window verbatim.
3. **Surfacing** context usage in the TUI status bar so the user always knows
   how full the window is.
4. **Exposing** a `--context-limit N` CLI flag (and `[context] limit = N` TOML
   key) so users can tune the threshold.

The implementation must stay true to the project's functional-first rules:
- No `mut` globals, no shared mutable state outside the `Arc<Mutex<Conversation>>` that already exists.
- `maybe_compress_context` is a pure-ish async function (its only side effect is an LLM call) — not a middleware closure, because `TurnMiddlewareFn` is sync.
- Every intermediate step must compile and pass `cargo test` before proceeding to the next.

---

## Technical Requirements

### New module: `src/context.rs`

```rust
use crate::provider::{Message, MessageContent, Provider, Role};

/// Heuristic token estimate for a single message (chars / 4, minimum 1).
pub fn estimate_message_tokens(msg: &Message) -> u32;

/// Sum of `estimate_message_tokens` across all messages.
pub fn estimate_tokens(messages: &[Message]) -> u32;

/// Find the index at which to split messages for summarisation.
///
/// Returns `Some(split)` where `messages[..split]` are the "old" messages to
/// summarise and `messages[split..]` are the recent messages to keep verbatim.
/// The split point is chosen so that `messages[split]` is always a `User`
/// message (required by the Bedrock alternating-turn constraint).
///
/// Returns `None` when the message list is too short to split or no `User`
/// message exists in the tail.
pub fn find_summary_split(messages: &[Message], keep_recent: usize) -> Option<usize>;

/// Summarise `old_messages` into a single String by calling the provider.
///
/// Streams the full response and returns the accumulated text.
pub async fn summarise_messages(
    old_messages: &[Message],
    provider: &dyn Provider,
) -> anyhow::Result<String>;

/// If estimated tokens exceed `limit * threshold`, summarise old messages and
/// return an updated Conversation plus `Some(ContextSummarized event)`.
/// Otherwise return the conversation unchanged and `None`.
pub async fn maybe_compress_context(
    conv: Conversation,
    provider: &dyn Provider,
    limit: u32,
    keep_recent: usize,
    threshold: f64,
) -> anyhow::Result<(Conversation, Option<TurnEvent>)>;
```

### `src/config.rs` additions

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    /// Hard token limit. `None` disables auto-summarisation.
    pub limit: Option<u32>,
    /// Number of recent messages to keep verbatim when summarising. Default 20.
    pub keep_recent_messages: usize,
    /// Fraction of `limit` at which to trigger summarisation. Default 0.80.
    pub summarize_threshold: f64,
}
```

Add `pub context: ContextConfig` to `AppConfig`; wire it in `overlay_from_table`.

### `src/main.rs` CLI flag

```rust
/// Maximum context tokens before auto-summarisation (e.g. 180000).
#[arg(long = "context-limit")]
context_limit: Option<u32>,
```

CLI value overrides `config.context.limit` when present.

### `src/types.rs` — new `TurnEvent` variant

```rust
TurnEvent::ContextSummarized {
    messages_before: usize,
    messages_after: usize,
    tokens_before: u32,
    tokens_after: u32,
},
```

### TUI changes (`src/tui/mod.rs`, `src/tui/ui.rs`)

- Add `last_input_tokens: u32` to `TuiApp` (the `input_tokens` from the most
  recent `Usage` event, not cumulative — represents current context size).
- Handle `TurnEvent::ContextSummarized` in `handle_ui_event`: push a synthetic
  `ChatEntry::AssistantDone` notice (e.g. `"[Context compressed: NNN → MMM messages]"`)
  and update `last_input_tokens` to `tokens_after`.
- Status bar: append `│ ctx: XX.Xk` always; when limit is configured also append
  ` / YYYk (ZZ%)` where `ZZ%` is `last_input_tokens / limit * 100`.

### Wiring in `main.rs`

Both paths call `maybe_compress_context` before `turn()`:

```rust
// headless
let (conv, summarize_event) = maybe_compress_context(
    conv_with_msg, provider.as_ref(), limit, keep_recent, threshold,
).await?;
if let Some(evt) = summarize_event { /* log to stderr */ }
let (updated_conv, events) = turn(conv, ...).await?;

// TUI (inside spawned task in handle_submit)
let c = conv_arc.lock().await.clone().with_user_message(trimmed);
let (c, summarize_event) = maybe_compress_context(c, ...).await
    .unwrap_or_else(|e| { /* log error, return original */ (c, None) });
if let Some(evt) = summarize_event { tx.send(evt).await.ok(); }
let (new_conv, events) = turn(c, ...).await;
```

---

## Ordered Implementation Steps

Each step must leave the project in a **compilable, test-passing state** before
starting the next.

---

### Step 1 — `src/context.rs`: pure token estimation

Create `src/context.rs` with the two estimation functions and `find_summary_split`.
No async, no I/O.  Register the module in `src/lib.rs`.

**Signatures:**
```rust
pub fn estimate_message_tokens(msg: &Message) -> u32
pub fn estimate_tokens(messages: &[Message]) -> u32
pub fn find_summary_split(messages: &[Message], keep_recent: usize) -> Option<usize>
```

**Token estimation rule:** sum `content_chars / 4` across all `MessageContent`
variants, minimum 1 per message:
- `Text { text }` → `text.len() as u32 / 4`
- `ToolUse { name, input, .. }` → `(name.len() + input.to_string().len()) as u32 / 4`
- `ToolResult { content, .. }` → `content.len() as u32 / 4`

**`find_summary_split` rule:**
- If `messages.len() <= keep_recent` → `None`
- `split = messages.len() - keep_recent`
- Scan `messages[split..]` for the first `Role::User` message
- Return `Some(split + position_of_first_user)` if found, else `None`

**Required tests (all in `src/context.rs`):**
- `estimate_tokens_empty` — zero messages → 0
- `estimate_tokens_text_message` — single text message, known char count
- `estimate_tokens_tool_use` — tool-use message, correct estimate
- `estimate_tokens_tool_result` — tool-result message
- `find_summary_split_too_short` — messages.len() <= keep_recent → None
- `find_summary_split_finds_user` — split lands on a User message
- `find_summary_split_skips_to_user` — oldest kept message is Assistant; split advances to first User
- `find_summary_split_no_user_in_tail` — None when tail has no User message

---

### Step 2 — `ContextConfig` in `AppConfig`

Add `ContextConfig` struct to `src/config.rs` with fields:
- `limit: Option<u32>` (default `None`)
- `keep_recent_messages: usize` (default `20`)
- `summarize_threshold: f64` (default `0.80`)

Add `pub context: ContextConfig` to `AppConfig`.

Extend `overlay_from_table` to handle a `[context]` TOML table:
- `limit` → `u32` (via `as_integer()`)
- `keep_recent_messages` → `usize`
- `summarize_threshold` → `f64`

**Required tests:**
- `context_config_defaults` — all defaults correct
- `context_config_toml_limit` — `[context]\nlimit = 100000` parsed correctly
- `context_config_toml_full` — all three keys parsed
- `context_config_missing_keys_preserve_defaults` — partial `[context]` table leaves unset keys at default
- `context_config_no_auto_summarize_when_limit_none` — default has `limit == None`

---

### Step 3 — `--context-limit` CLI flag

In `src/main.rs`:
- Add `context_limit: Option<u32>` to `Args`
- After `AppConfig::load()`, override: if `args.context_limit.is_some()`, set
  `config.context.limit = args.context_limit`
- Pass `config.context.limit` through to where `turn()` is called (store it as a
  local; do not call `maybe_compress_context` yet — that comes in Step 6)

No new tests required for this step (CLI parsing is integration-tested elsewhere).
`cargo build` must succeed.

---

### Step 4 — `TurnEvent::ContextSummarized` + TUI wiring + status bar

**`src/types.rs`:** Add to `TurnEvent`:
```rust
/// Emitted when old messages are replaced by a summary.
ContextSummarized {
    messages_before: usize,
    messages_after: usize,
    tokens_before: u32,
    tokens_after: u32,
},
```

**`src/tui/mod.rs`:**
- Add `pub last_input_tokens: u32` to `TuiApp` (default `0`).
- In `headless()` constructor, set it to `0`.
- In `handle_ui_event`:
  - `TurnEvent::Usage { input_tokens, .. }` → also set `self.last_input_tokens = input_tokens`
    (this replaces, not accumulates — it tracks the *most recent* input size).
  - `TurnEvent::ContextSummarized { messages_before, messages_after, tokens_before, tokens_after }` →
    push a `ChatEntry::AssistantDone(vec![ChatBlock::Text(format!(...))])` notice and
    set `self.last_input_tokens = tokens_after`.

**`src/tui/ui.rs`:** Extend `render_status_bar`:
- Always show `│ ctx: {last_k:.1}k` where `last_k = app.last_input_tokens as f64 / 1000.0`.
- When `app.context_limit` (a new field — see below) is `Some(limit)`:
  append `/{limit_k:.0}k ({pct:.0}%)`.
- Add `pub context_limit: Option<u32>` to `TuiApp` (set from `config.context.limit`
  when wired in Step 7; default `None` for now — the field just needs to exist).

**Required tests:**
- `turn_event_context_summarized_clonable` — new variant roundtrips through clone
- `handle_ui_event_context_summarized_appends_notice` — chat_history grows by 1
- `handle_ui_event_usage_updates_last_input_tokens` — `last_input_tokens` reflects latest `input_tokens`
- `handle_ui_event_usage_still_accumulates_totals` — `total_input_tokens` still accumulates
- `status_bar_ctx_display_no_limit` — formatted `ctx: 45.2k` when limit is None
- `status_bar_ctx_display_with_limit` — formatted `ctx: 45.2k/200k (23%)` when limit is Some

---

### Step 5 — `src/context.rs`: async summarisation functions

Add to `src/context.rs`:

```rust
use crate::provider::{Provider, Role};
use crate::types::{Conversation, TurnEvent};
use futures::StreamExt;

pub async fn summarise_messages(
    old_messages: &[Message],
    provider: &dyn Provider,
) -> anyhow::Result<String>
```

**`summarise_messages` implementation:**
- Build a single-message list:
  ```
  User: "Summarise the following conversation history concisely. Capture key
  decisions, code changes, file paths modified, tool results, and any important
  context needed to continue the work.\n\n{formatted}"
  ```
  where `formatted` is each message rendered as `[Role] content\n`.
- Call `provider.stream_completion(&msgs, &[], None)` and drain the stream,
  accumulating `TextDelta` fragments.
- Return the accumulated string, or `Err` if the stream yields an error.

```rust
pub async fn maybe_compress_context(
    conv: Conversation,
    provider: &dyn Provider,
    limit: u32,
    keep_recent: usize,
    threshold: f64,
) -> anyhow::Result<(Conversation, Option<TurnEvent>)>
```

**`maybe_compress_context` implementation:**
1. `estimated = estimate_tokens(&conv.messages)`
2. If `estimated < (limit as f64 * threshold) as u32` → return `(conv, None)` (fast path)
3. `split = find_summary_split(&conv.messages, keep_recent)` → if `None` → return `(conv, None)` (cannot split)
4. `old = &conv.messages[..split]`; `recent = conv.messages[split..].to_vec()`
5. `summary = summarise_messages(old, provider).await?`
6. Build `new_messages`:
   ```rust
   let mut new_messages = vec![Message::user(
       format!("[Earlier conversation summary]\n{summary}")
   )];
   new_messages.extend(recent);
   ```
7. `tokens_after = estimate_tokens(&new_messages)`
8. Build `TurnEvent::ContextSummarized { messages_before: conv.messages.len(), messages_after: new_messages.len(), tokens_before: estimated, tokens_after }`
9. Build new `Conversation` with `new_messages` (clone `conv`, replace `messages` field).
10. Return `(new_conv, Some(event))`

**Required tests (async, use `#[tokio::test]`):**
- `summarise_messages_collects_stream` — MockProvider returns two TextDelta + TurnEnd; result equals concatenated text
- `summarise_messages_provider_error_returns_err` — ErrorProvider → `Err`
- `maybe_compress_context_no_op_under_threshold` — estimated < threshold → returns same conv, None
- `maybe_compress_context_compresses_when_over_threshold` — estimated >= threshold → messages shortened, event is Some
- `maybe_compress_context_new_messages_start_with_user` — first message in result is always User
- `maybe_compress_context_cannot_split_returns_unchanged` — too few messages → returns unchanged

Use the same `MockProvider` / `ErrorProvider` pattern already in `src/turn.rs` tests
(copy the helper structs into a `#[cfg(test)]` block in `context.rs`).

---

### Step 6 — Wire `maybe_compress_context` in the headless path

In `run_headless` in `src/main.rs`, after building `conv_with_msg` and before
calling `turn()`:

```rust
use ap::context::maybe_compress_context;

let conv_with_msg = conv.with_user_message(prompt.to_string());

let (conv_with_msg, summarize_event) = if let Some(limit) = config.context.limit {
    match maybe_compress_context(
        conv_with_msg,
        provider.as_ref(),
        limit,
        config.context.keep_recent_messages,
        config.context.summarize_threshold,
    )
    .await
    {
        Ok(result) => result,
        Err(e) => {
            eprintln!("ap: warning: context compression failed: {e}");
            (conv_with_msg_original, None) // fall through with original
        }
    }
} else {
    (conv_with_msg, None)
};

if let Some(TurnEvent::ContextSummarized { messages_before, messages_after, tokens_before, tokens_after }) = summarize_event {
    eprintln!(
        "ap: context compressed: {messages_before} → {messages_after} messages \
         ({tokens_before} → {tokens_after} estimated tokens)"
    );
}
```

`cargo build` and all existing tests must still pass.

---

### Step 7 — Wire `maybe_compress_context` in the TUI path + set `context_limit` field

**`src/tui/mod.rs`:**

In `TuiApp::new`, accept `context_limit: Option<u32>` as the last parameter and
store it.

In the spawned task inside `handle_submit`, replace the current:
```rust
let c = conv_arc.lock().await.clone().with_user_message(trimmed);
match turn(c, ...).await { ... }
```

with:
```rust
let c = conv_arc.lock().await.clone().with_user_message(trimmed.clone());

// Context compression (only when limit is configured)
let (c, summarize_event) = if let Some(limit) = context_limit {
    match ap::context::maybe_compress_context(
        c, &*provider, limit, keep_recent, threshold,
    ).await {
        Ok(pair) => pair,
        Err(e) => {
            let _ = tx.send(TurnEvent::Error(format!("context compression: {e}"))).await;
            return;
        }
    }
} else {
    (c, None)
};
if let Some(evt) = summarize_event {
    let _ = tx.send(evt).await;
}

match turn(c, &*provider, &tools, &middleware).await { ... }
```

Capture the necessary `context_limit`, `keep_recent`, `threshold` values by cloning
from `Arc`-wrapped config or by copying the `u32`/`usize`/`f64` scalars into the
closure (they are `Copy`).

**`src/main.rs`:** In `run_tui`, pass `config.context.limit` to `TuiApp::new`.

**Required tests:**
- `tuiapp_new_stores_context_limit` — construct a headless TuiApp with a context
  limit and verify the field is stored (adapt `TuiApp::headless()` to accept an
  optional limit, or add a `headless_with_limit` constructor).
- Ensure all existing TUI tests still pass.

---

## Acceptance Criteria

All of the following must be true before outputting `LOOP_COMPLETE`:

1. `cargo build --release` completes with zero errors and zero warnings.
2. `cargo test` passes with zero failures.
3. `cargo clippy -- -D warnings` produces no warnings or errors.
4. The `src/context.rs` module exists and exports:
   - `estimate_tokens`, `estimate_message_tokens`, `find_summary_split`
   - `summarise_messages`, `maybe_compress_context`
5. `AppConfig` has a `context: ContextConfig` field with correct defaults
   (`limit: None`, `keep_recent_messages: 20`, `summarize_threshold: 0.80`).
6. `TurnEvent::ContextSummarized { messages_before, messages_after, tokens_before, tokens_after }`
   exists and is `Clone`.
7. `TuiApp` has `last_input_tokens: u32` and `context_limit: Option<u32>` fields.
8. The TUI status bar renders `ctx: XX.Xk` unconditionally and appends `/YYYk (ZZ%)`
   when `context_limit` is `Some`.
9. Running `ap --context-limit 50000 -p "hello"` does not panic or fail to compile.
10. `maybe_compress_context` returns the conversation unchanged when:
    - `limit` is satisfied (estimated tokens below threshold), or
    - The message list is too short to split.
11. When summarisation fires, the first message of the resulting conversation is
    a `Role::User` message (Bedrock alternating-turn constraint).
12. All new tests from Steps 1–7 are present and pass.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project builds clean.
