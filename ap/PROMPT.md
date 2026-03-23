# Provider Abstraction — Implementation PROMPT

## Vision

`ap` currently hard-codes AWS Bedrock as the only LLM backend. The goal is to
introduce a clean **provider abstraction** so any OpenAI-compatible endpoint
(OpenRouter, LM Studio, Ollama, direct OpenAI) can be used by changing two
lines in `ap.toml` — no recompilation, no code changes.

The existing `Provider` trait in `src/provider/mod.rs` is already well-designed
for this. This work adds:

1. An `openai-compat` backend in `src/provider/openai.rs` that speaks the
   OpenAI Chat Completions streaming API (SSE `data:` lines, `delta.content` /
   `delta.tool_calls`).
2. Config wiring: `[provider] backend = "openai-compat"` picks the new
   provider; `base_url` and `api_key` are read from `ProviderConfig`.
3. Tool-call format parity: the `openai-compat` provider emits the same
   `StreamEvent` sequence as `BedrockProvider` — `ToolUseStart` /
   `ToolUseParams` / `ToolUseEnd` — so `turn()` requires **zero changes**.
4. A `build_provider` factory function that reads `AppConfig` and returns
   `Arc<dyn Provider>`, replacing the two ad-hoc `BedrockProvider::new()`
   calls in `main.rs`.

---

## Technical Requirements

### New config fields (`src/config.rs`)

Extend `ProviderConfig`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub backend: String,   // "bedrock" (default) | "openai-compat"
    pub model: String,
    pub region: String,    // Bedrock only
    pub base_url: String,  // openai-compat: e.g. "https://openrouter.ai/api/v1"
    pub api_key: String,   // openai-compat: Bearer token (empty string = no auth)
}
```

Defaults for new fields: `base_url = ""`, `api_key = ""`.
`overlay_from_table` must handle the two new keys identically to the existing
`backend` / `model` / `region` keys (explicit-only overlay, no serde default
bleed-through).

---

### New provider (`src/provider/openai.rs`)

```rust
pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    base_url: String,   // trailing slash stripped at construction
    api_key: String,
    model: String,
}

impl OpenAiCompatProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self { ... }

    /// Convert ap's Message/MessageContent slice into the OpenAI messages array.
    /// ToolResult content → role "tool", tool_call_id set.
    /// ToolUse → role "assistant" with tool_calls array.
    fn build_messages(messages: &[Message]) -> Vec<serde_json::Value> { ... }

    /// Convert ap's tool schemas (already in Anthropic tool_use JSON) into
    /// OpenAI `tools` array format ({ type: "function", function: { name, description, parameters } }).
    fn build_tools(tools: &[serde_json::Value]) -> Vec<serde_json::Value> { ... }

    /// Parse one SSE `data:` line (the JSON string after stripping "data: ").
    /// Returns zero or more StreamEvents. Mirrors BedrockProvider::parse_sse_event.
    pub fn parse_sse_line(
        line: &str,
        state: &mut OpenAiStreamState,
        out: &mut Vec<Result<StreamEvent, ProviderError>>,
    ) { ... }
}

impl Provider for OpenAiCompatProvider { ... }
```

**`OpenAiStreamState`** — tracks in-progress tool call accumulation across SSE
chunks:

```rust
#[derive(Debug, Default)]
pub struct OpenAiStreamState {
    /// Index of the currently-open tool_call slot (-1 = none / text mode).
    pub current_tool_index: i64,
    /// Accumulated name for the current tool call (arrives in first chunk).
    pub current_tool_name: String,
    /// Accumulated id for the current tool call.
    pub current_tool_id: String,
    /// Whether we have emitted ToolUseStart for the current slot.
    pub tool_start_emitted: bool,
}
```

**SSE parsing rules** (OpenAI streaming format):

```
data: {"id":"...","object":"chat.completion.chunk","choices":[{
  "delta": {
    "content": "hello"           // → StreamEvent::TextDelta("hello")
  }
}]}

data: {"choices":[{"delta":{
  "tool_calls":[{
    "index": 0,
    "id": "call_abc",            // present in first chunk for this index
    "function": {
      "name": "bash",            // present in first chunk
      "arguments": "{\"cmd\":"   // may be partial; accumulate → ToolUseParams
    }
  }]
}}]}

data: [DONE]                     // → StreamEvent::TurnEnd (stop_reason "end_turn",
                                 //   tokens from x_usage if present, else 0)
```

Rules:
- When `delta.tool_calls[n].index` changes to a new value AND we were
  accumulating a previous tool call, emit `StreamEvent::ToolUseEnd` for the
  old one, then `StreamEvent::ToolUseStart` for the new one.
- When a chunk carries `delta.tool_calls[n]` with the same index as current,
  emit `StreamEvent::ToolUseParams(arguments_fragment)`.
- The `id` and `name` fields only appear in the **first** chunk for each
  `index`; subsequent chunks for the same index only carry `arguments`.
- On `data: [DONE]`: if `tool_start_emitted` is true, emit `ToolUseEnd` first,
  then `TurnEnd`.
- Token counts: look for `usage.prompt_tokens` / `usage.completion_tokens` on
  the `[DONE]` chunk or any chunk that carries a `usage` field. If absent,
  emit 0/0.
- `finish_reason: "tool_calls"` on a non-DONE chunk should also trigger
  `ToolUseEnd` if a tool is open, but do NOT emit `TurnEnd` (wait for `[DONE]`).

**`Provider` impl** — `stream_completion` must:
1. Call `POST {base_url}/chat/completions` with `stream: true`.
2. Set `Authorization: Bearer {api_key}` header only when `api_key` is
   non-empty.
3. Read the response as a byte stream, splitting on `\n`.
4. Forward each `data: ...` line to `parse_sse_line`.
5. Return a `BoxStream<'a, Result<StreamEvent, ProviderError>>`.

Errors:
- Non-2xx HTTP response → `ProviderError::Aws(format!("HTTP {status}: {body}"))`.
  (Reuse `ProviderError::Aws` — no new variant needed.)
- Reqwest transport error → `ProviderError::Aws(e.to_string())`.
- JSON parse failure → `ProviderError::ParseError(...)`.

---

### Provider factory (`src/provider/mod.rs`)

```rust
/// Build the appropriate provider from config, returning an Arc<dyn Provider>.
/// Called once at startup in main.rs.
pub async fn build_provider(config: &AppConfig) -> anyhow::Result<Arc<dyn Provider>>;
```

Logic:

```
match config.provider.backend.as_str() {
    "openai-compat" => Arc::new(OpenAiCompatProvider::new(
        &config.provider.base_url,
        &config.provider.api_key,
        &config.provider.model,
    )),
    _ /* "bedrock" */ => Arc::new(
        BedrockProvider::new(&config.provider.model, &config.provider.region).await?
    ),
}
```

---

### `main.rs` changes

Replace the two `BedrockProvider::new(...)` call-sites with `build_provider(&config).await?`.
Remove the `use ap::provider::BedrockProvider;` import.
`run_headless` and `run_tui` both call `build_provider`.

---

### `ap.toml.example` additions

```toml
# OpenAI-compatible provider example (uncomment and fill in to use)
# backend = "openai-compat"
# base_url = "https://openrouter.ai/api/v1"
# api_key  = "sk-or-..."
# model    = "anthropic/claude-3.5-sonnet"
```

---

## Ordered Implementation Steps

Each step must leave `cargo build` (and `cargo test --lib`) **green** before
moving to the next.

---

### Step 1 — Extend `ProviderConfig` with `base_url` and `api_key`

**Files:** `src/config.rs`

1. Add `base_url: String` and `api_key: String` to `ProviderConfig`.
2. Set defaults: `base_url = String::new()`, `api_key = String::new()`.
3. Extend `overlay_from_table` to overlay `base_url` and `api_key` when the
   respective keys are present in the TOML table.

**New tests** (add to `src/config.rs` `#[cfg(test)]` block):

```rust
#[test]
fn provider_config_new_fields_default_empty() {
    let cfg = AppConfig::default();
    assert_eq!(cfg.provider.base_url, "");
    assert_eq!(cfg.provider.api_key, "");
}

#[test]
fn provider_config_base_url_overlay() {
    // Write a TOML file with base_url set; api_key absent.
    // Verify base_url is overridden, api_key stays "".
}

#[test]
fn provider_config_api_key_overlay() {
    // Write a TOML file with api_key set; base_url absent.
    // Verify api_key is overridden, base_url stays "".
}

#[test]
fn provider_config_both_fields_overlay() {
    // Both base_url and api_key present in TOML.
}
```

`cargo test --lib` must pass.

---

### Step 2 — Skeleton `OpenAiCompatProvider` (compiles, no HTTP)

**Files:** `src/provider/openai.rs` (new), `src/provider/mod.rs`

1. Create `src/provider/openai.rs` with:
   - `pub struct OpenAiStreamState` (all fields as specified).
   - `pub struct OpenAiCompatProvider` with `client`, `base_url`, `api_key`,
     `model` fields.
   - `impl OpenAiCompatProvider { pub fn new(...) -> Self }`.
   - `pub fn parse_sse_line(line: &str, state: &mut OpenAiStreamState, out: &mut Vec<...>)`.
     Body: stub that does nothing (empty). 
   - `impl Provider for OpenAiCompatProvider` — `stream_completion` returns
     `stream::empty().boxed()` (placeholder, always yields nothing).

2. In `src/provider/mod.rs`:
   - Add `pub mod openai;` and `pub use openai::OpenAiCompatProvider;`.

**Acceptance:** `cargo build` green. No tests needed for this step beyond
compilation.

---

### Step 3 — SSE parsing: text delta and `[DONE]`

**Files:** `src/provider/openai.rs`

Implement `parse_sse_line` for the two simplest cases:
- `data: [DONE]` → emit `StreamEvent::TurnEnd { stop_reason: "end_turn", input_tokens: 0, output_tokens: 0 }`.
- `data: {...}` where `delta.content` is a non-null string → emit `StreamEvent::TextDelta(...)`.
- Anything else (empty line, `event:`, unknown JSON shape) → no-op.

**New unit tests** in `src/provider/openai.rs`:

```rust
fn parse(line: &str, state: &mut OpenAiStreamState) -> Vec<StreamEvent> { ... }

#[test]
fn parse_done_emits_turn_end() { ... }

#[test]
fn parse_text_delta_emits_text_chunk() { ... }

#[test]
fn parse_empty_line_is_noop() { ... }

#[test]
fn parse_event_prefix_line_is_noop() { ... }
```

`cargo test --lib` must pass.

---

### Step 4 — SSE parsing: tool call accumulation

**Files:** `src/provider/openai.rs`

Extend `parse_sse_line` to handle `delta.tool_calls`:

- First chunk for `index == 0` (or any new index): emit `ToolUseStart { id, name }`, set `tool_start_emitted = true`.
- Subsequent chunks for same index: emit `ToolUseParams(arguments_fragment)`.
- Index change from N to M (M > N): emit `ToolUseEnd` for N, then `ToolUseStart` for M.
- `data: [DONE]` with `tool_start_emitted == true`: emit `ToolUseEnd`, then `TurnEnd`.

**New unit tests:**

```rust
#[test]
fn parse_tool_use_single_call() {
    // Three chunks: first (id+name+partial_args), second (more args), [DONE].
    // Expected: ToolUseStart, ToolUseParams×2, ToolUseEnd, TurnEnd.
}

#[test]
fn parse_tool_use_two_sequential_calls() {
    // Chunks for index 0, then index 1, then [DONE].
    // Expected: ToolUseStart(0), params, ToolUseEnd(0),
    //           ToolUseStart(1), params, ToolUseEnd(1), TurnEnd.
}

#[test]
fn parse_done_without_open_tool_no_extra_end() {
    // [DONE] after pure text: only TurnEnd, no ToolUseEnd.
}

#[test]
fn parse_tool_params_accumulate_in_order() {
    // Two ToolUseParams events must arrive in the order the SSE chunks arrive.
}

#[test]
fn parse_finish_reason_tool_calls_closes_tool() {
    // A non-DONE chunk with finish_reason = "tool_calls" should emit ToolUseEnd
    // if a tool is open, but NOT TurnEnd.
}
```

`cargo test --lib` must pass.

---

### Step 5 — Token count extraction

**Files:** `src/provider/openai.rs`

When `parse_sse_line` sees a `usage` object (either on the DONE chunk or any
chunk that carries it), read `usage.prompt_tokens` → `input_tokens` and
`usage.completion_tokens` → `output_tokens` into the `TurnEnd` event.

Store the last-seen token counts in `OpenAiStreamState`:

```rust
pub struct OpenAiStreamState {
    // ...existing fields...
    pub input_tokens: u32,
    pub output_tokens: u32,
}
```

**New tests:**

```rust
#[test]
fn parse_usage_on_done_chunk() {
    // data: {"usage":{"prompt_tokens":10,"completion_tokens":5},...}
    // followed by data: [DONE]
    // TurnEnd carries input_tokens=10, output_tokens=5.
}

#[test]
fn parse_usage_absent_defaults_zero() {
    // Plain [DONE] with no usage field → TurnEnd with 0/0.
}
```

`cargo test --lib` must pass.

---

### Step 6 — Message format conversion (`build_messages` / `build_tools`)

**Files:** `src/provider/openai.rs`

Implement the two static helpers:

**`build_messages`** — convert `&[Message]` to OpenAI format:

| ap `MessageContent` variant | OpenAI representation |
|---|---|
| `Text { text }` on User msg | `{ role: "user", content: text }` |
| `Text { text }` on Assistant msg | `{ role: "assistant", content: text }` |
| `ToolUse { id, name, input }` | role `"assistant"`, no `content`, `tool_calls: [{ id, type: "function", function: { name, arguments: json_string } }]` |
| `ToolResult { tool_use_id, content, .. }` | `{ role: "tool", tool_call_id, content }` |

Rules:
- A single assistant `Message` may contain both `Text` and `ToolUse` content
  blocks. In that case emit one OpenAI message with both `content` (the text)
  and `tool_calls` (the tool_use blocks).
- A user `Message` with only `ToolResult` blocks becomes multiple `role: "tool"`
  messages (one per result) — OpenAI expects each tool result as its own
  message.

**`build_tools`** — convert ap's Anthropic-format tool schemas to OpenAI format:

Input schema shape (Anthropic): `{ name, description, input_schema: { type, properties, required } }`
Output shape (OpenAI): `{ type: "function", function: { name, description, parameters: { type, properties, required } } }`

**New tests:**

```rust
#[test]
fn build_messages_user_text() { ... }

#[test]
fn build_messages_assistant_text() { ... }

#[test]
fn build_messages_tool_use_assistant() {
    // ToolUse block → tool_calls array, arguments is a JSON string.
}

#[test]
fn build_messages_tool_result_user() {
    // ToolResult block → role "tool" message.
}

#[test]
fn build_messages_mixed_assistant() {
    // Assistant message with Text + ToolUse → single message with content + tool_calls.
}

#[test]
fn build_tools_converts_schema() {
    // Anthropic input_schema → OpenAI parameters.
}
```

`cargo test --lib` must pass.

---

### Step 7 — HTTP streaming in `stream_completion`

**Files:** `src/provider/openai.rs`

Replace the placeholder `stream_completion` body with real HTTP logic:

```
POST {self.base_url}/chat/completions
Content-Type: application/json
Authorization: Bearer {api_key}   // only when api_key is non-empty

Body: {
  "model": self.model,
  "stream": true,
  "stream_options": { "include_usage": true },   // requests usage on [DONE]
  "messages": build_messages(messages),
  "tools": build_tools(tools)      // omit "tools" key when tools is empty
}
```

Stream reading:
1. Check HTTP status; if non-2xx read body to string and return
   `ProviderError::Aws(format!("HTTP {status}: {body}"))`.
2. Read response bytes via `response.bytes_stream()`.
3. Accumulate bytes into a line buffer; on each `\n`, call `parse_sse_line`.
4. After the byte stream ends, if no `TurnEnd` was emitted yet, emit one
   (handles servers that omit `[DONE]`).
5. Yield each event via a `futures::channel::mpsc` or `stream::unfold` pattern.

The returned `BoxStream` must be `'a` (no `'static` requirement).

**No new unit tests for this step** (requires a live HTTP server). The
existing `parse_sse_line` unit tests cover the parsing layer. Integration is
verified manually or via acceptance criteria below.

`cargo build` must pass.

---

### Step 8 — `build_provider` factory + `main.rs` wiring

**Files:** `src/provider/mod.rs`, `src/main.rs`, `ap.toml.example`

1. Add to `src/provider/mod.rs`:

```rust
use std::sync::Arc;
use crate::config::AppConfig;

pub async fn build_provider(config: &AppConfig) -> anyhow::Result<Arc<dyn Provider>> {
    match config.provider.backend.as_str() {
        "openai-compat" => {
            Ok(Arc::new(OpenAiCompatProvider::new(
                &config.provider.base_url,
                &config.provider.api_key,
                &config.provider.model,
            )))
        }
        _ => {
            let p = BedrockProvider::new(
                &config.provider.model,
                &config.provider.region,
            )
            .await?;
            Ok(Arc::new(p))
        }
    }
}
```

2. In `src/main.rs`:
   - Remove `use ap::provider::BedrockProvider;`.
   - Add `use ap::provider::build_provider;`.
   - Replace both `BedrockProvider::new(...)` blocks in `run_headless` and
     `run_tui` with `build_provider(&config).await?` (or equivalent with
     error handling matching existing style).

3. In `ap.toml.example`: add commented-out `openai-compat` example block
   under `[provider]`.

**New test** (add to `src/provider/mod.rs` `#[cfg(test)]`):

```rust
#[tokio::test]
async fn build_provider_bedrock_default() {
    // Default config → Bedrock provider constructs without panic.
    let cfg = AppConfig::default();
    let result = build_provider(&cfg).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn build_provider_openai_compat() {
    // openai-compat backend → constructs synchronously, no network call.
    let mut cfg = AppConfig::default();
    cfg.provider.backend = "openai-compat".to_string();
    cfg.provider.base_url = "http://localhost:1234/v1".to_string();
    cfg.provider.model = "llama3".to_string();
    let result = build_provider(&cfg).await;
    assert!(result.is_ok());
}
```

`cargo test --lib` and `cargo build` must both pass clean.

---

## Acceptance Criteria

All of the following must be true for the loop to be considered complete:

1. **`cargo build` is clean** — zero errors, zero warnings (the project's
   existing `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`
   denials must continue to pass).

2. **`cargo test --lib` passes** — all existing tests continue to pass; all
   new tests added in Steps 1–8 pass.

3. **`ProviderConfig` has `base_url` and `api_key` fields** with empty-string
   defaults that do not appear in serialized output when unset, and that are
   correctly overlaid from TOML config files.

4. **`OpenAiCompatProvider` exists** and implements `Provider`. It is exported
   from `src/provider/mod.rs` as `pub use openai::OpenAiCompatProvider`.

5. **`parse_sse_line` correctly handles** all of: text deltas, single tool
   call, two sequential tool calls, `[DONE]` with and without an open tool,
   token counts present and absent.

6. **`build_messages`** correctly converts all four `MessageContent` variants
   to OpenAI format, including mixed assistant messages (text + tool_calls).

7. **`build_tools`** correctly converts Anthropic input_schema format to
   OpenAI parameters format.

8. **`build_provider` factory** exists in `src/provider/mod.rs`, selects
   `OpenAiCompatProvider` for `backend = "openai-compat"` and `BedrockProvider`
   for `backend = "bedrock"` (or any unrecognised value).

9. **`main.rs` uses `build_provider`** — the two `BedrockProvider::new`
   call-sites are gone; the file has no direct import of `BedrockProvider`.

10. **`ap.toml.example`** contains a commented-out `openai-compat` example
    block showing `backend`, `base_url`, `api_key`, and `model`.

11. **`turn()` is unchanged** — no modifications to `src/turn.rs`. The
    `Provider` trait contract (`stream_completion` returning the same
    `StreamEvent` sequence) is the only integration point.

12. **The `reqwest` dependency** (already in `Cargo.toml`) is used by
    `OpenAiCompatProvider` with the `stream` feature. Add
    `features = ["json", "stream"]` to the `reqwest` entry in `Cargo.toml`
    if the `stream` feature is not already present.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project
builds clean.
