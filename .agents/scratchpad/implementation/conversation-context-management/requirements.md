# Requirements — Conversation Context Management

## Source

Derived from `PROMPT.md` at `/Users/sam.painter/Projects/ap/PROMPT.md` and Q&A clarification.

## Functional Requirements

### FR-1: Token Estimation (pure, synchronous)
- Heuristic: `chars / 4`, minimum 1 per message
- Covers `Text`, `ToolUse`, and `ToolResult` `MessageContent` variants
- Exported as `estimate_message_tokens(msg: &Message) -> u32` and `estimate_tokens(messages: &[Message]) -> u32`

### FR-2: Summary Split (pure, synchronous)
- `find_summary_split(messages: &[Message], keep_recent: usize) -> Option<usize>`
- Returns `None` when `messages.len() <= keep_recent`
- Advances split forward until first `Role::User` message in tail (Bedrock alternating-turn constraint)
- Returns `None` when no `User` message exists in the tail

### FR-3: Summarisation (async, I/O)
- `summarise_messages(old_messages: &[Message], provider: &dyn Provider) -> anyhow::Result<String>`
- Builds a prompt asking the LLM to summarise key decisions, code changes, file paths, tool results
- Streams response via `provider.stream_completion`, accumulates `TextDelta` fragments

### FR-4: Context Compression Orchestrator (async)
- `maybe_compress_context(conv, provider, limit, keep_recent, threshold) -> anyhow::Result<(Conversation, Option<TurnEvent>)>`
- Fast path: returns `(conv, None)` when estimated tokens < `limit * threshold`
- Calls `find_summary_split`; if `None`, returns unchanged
- Constructs `new_messages` = `[User("[Earlier conversation summary]\n{summary}")]` + recent tail
- Returns new `Conversation` (via `with_messages` builder) plus `Some(TurnEvent::ContextSummarized)`

### FR-5: Config Extension
- `ContextConfig { limit: Option<u32>, keep_recent_messages: usize, summarize_threshold: f64 }`
- Defaults: `limit = None`, `keep_recent_messages = 20`, `summarize_threshold = 0.80`
- Added to `AppConfig` as `pub context: ContextConfig`
- TOML key: `[context]` with `limit`, `keep_recent_messages`, `summarize_threshold`

### FR-6: CLI Flag
- `--context-limit <N>: Option<u32>` on `Args`
- Overrides `config.context.limit` when present

### FR-7: New TurnEvent Variant
- `TurnEvent::ContextSummarized { messages_before, messages_after, tokens_before, tokens_after }`
- Must be `Clone`

### FR-8: TUI Fields
- `TuiApp.last_input_tokens: u32` — tracks most recent input token count from `Usage` event (not cumulative)
- `TuiApp.context_limit: Option<u32>` — set from config

### FR-9: TUI Status Bar
- Always displays `│ ctx: XX.Xk`
- When `context_limit` is `Some(limit)`, also displays `/ YYYk (ZZ%)`

### FR-10: TUI Event Handling
- `Usage` event → update `last_input_tokens = input_tokens` (replaces, not accumulates)
- `ContextSummarized` → push notice `ChatEntry::AssistantDone` + update `last_input_tokens = tokens_after`

### FR-11: Headless Path Wiring
- Call `maybe_compress_context` before `turn()` in `run_headless`
- Graceful fallback on compression error (log to stderr, continue with original conversation)
- Log summary to stderr when compression fires

### FR-12: TUI Path Wiring
- Call `maybe_compress_context` in spawned task before `turn()` in `handle_submit`
- On error: send `TurnEvent::Error`, return early
- Forward `ContextSummarized` event via channel before calling `turn()`

## Non-Functional Requirements

### NFR-1: Functional-first style
- `mut` in local consuming builders is acceptable; no call-site `let mut` accumulation
- `Conversation::with_messages(Vec<Message>) -> Self` builder must be added (clarification from Q&A)
- No shared mutable state

### NFR-2: Incremental compilability
- Each of the 7 steps must leave the project in compilable, test-passing state

### NFR-3: Zero warnings
- `cargo build --release`, `cargo test`, `cargo clippy -- -D warnings` all must pass clean

## Clarifications (from Q&A)

- **`Conversation::with_messages`**: Add as a consuming builder (`pub fn with_messages(mut self, messages: Vec<Message>) -> Self`). This follows the existing `with_user_message`/`with_system_prompt` pattern. `mut self` inside the method body is acceptable; the AGENTS.md warning targets call-site `let mut` bindings. Confidence: 95.

- **Clone-before-call (ownership)**: `maybe_compress_context` takes owned `Conversation`. The call site **must** clone `conv_with_msg` before the call and keep the clone for the `Err` fallback branch. Rust's ownership rules make any other approach a compile error. Required pattern:
  ```rust
  let fallback = conv_with_msg.clone();
  match maybe_compress_context(conv_with_msg, ...).await {
      Ok((compressed, event)) => { /* use compressed */ }
      Err(e) => { eprintln!("warning: {e}"); /* use fallback */ }
  }
  ```

- **`TuiApp::headless_with_limit`**: `TuiApp::headless()` has 23 existing test call sites. These must **not** be touched. Instead add `pub fn headless_with_limit(context_limit: Option<u32>) -> Self` as the new test helper. `headless()` remains unchanged and internally delegates with `None`. The `tuiapp_new_stores_context_limit` Step 7 test uses `headless_with_limit`.

- **Test count**: Spec defines **26 tests** (Step 1: 8, Step 2: 5, Step 4: 6, Step 5: 6, Step 7: 1). The design must specify 26, not 25.
