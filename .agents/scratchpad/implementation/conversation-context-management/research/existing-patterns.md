# Existing Patterns — conversation-context-management

## Conversation Builder Pattern
**File:** `ap/src/types.rs:40-67`

```rust
pub fn with_user_message(mut self, content: impl Into<String>) -> Self {
    self.messages.push(Message { role: Role::User, content: ... });
    self
}

pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
    self.system_prompt = Some(prompt.into());
    self
}
```

Pattern: `mut self` consuming builder — the `mut` is inside the method body, not at
call-site. AGENTS.md says this is fine; the warning targets call-site `let mut`.

`with_messages(Vec<Message>) -> Self` follows this exact pattern.

## Config Sub-struct Pattern
**File:** `ap/src/config.rs`

`SkillsConfig` is the closest analogue to the new `ContextConfig`. Key observations:
- `SkillsConfig` is NOT `#[serde(default)]` on the struct itself but uses `#[serde(skip)]` on `AppConfig.skills`
- ACTUALLY: `SkillsConfig` uses `#[serde(skip)]` on the field in `AppConfig` (line ~84)
- Overlay happens in `overlay_from_table()` via explicit key checking in the `skills` table section
- `ContextConfig` should follow the `SkillsConfig` approach:
  - Add `#[derive(Debug, Clone, Serialize, Deserialize)]` with `#[serde(default)]`
  - OR use `#[serde(skip)]` like `SkillsConfig` if it shouldn't serialize to session files
  
IMPORTANT: `SkillsConfig` is NOT `Serialize/Deserialize` and is `#[serde(skip)]` in `AppConfig`.
For `ContextConfig` the design says `serde(default)` — it would serialize/deserialize (like `ProviderConfig`).
But `AppConfig` is stored in `Conversation` which is session-persisted. Adding `context` to session files
is probably fine (default values if absent on load = backward compat via `#[serde(default)]`).

## overlay_from_table Pattern for skills
**File:** `ap/src/config.rs:106-131`

```rust
if let Some(toml::Value::Table(st)) = table.get("skills") {
    if st.contains_key("enabled") {
        if let Some(toml::Value::Boolean(v)) = st.get("enabled") {
            base.skills.enabled = *v;
        }
    }
    if st.contains_key("max_injected") { ... }
}
```

`ContextConfig` overlay follows the same pattern. Fields: `limit` (optional u32), 
`keep_recent_messages` (usize, default 20), `summarize_threshold` (f64, default 0.8).

For `limit`: `toml::Value::Integer(v)` → `u32::try_from(*v).ok()` → `Some(v)`.

## TurnEvent Variants
**File:** `ap/src/types.rs:73-97`

All variants derive `Debug, Clone`. New `ContextSummarized` variant follows the same pattern.
No `PartialEq` on `TurnEvent` (it uses `serde_json::Value` in `ToolStart` which doesn't impl `PartialEq`).

## MockProvider in tests
**File:** `ap/src/turn.rs:241-286`

```rust
struct MockProvider {
    scripts: Arc<Mutex<VecDeque<Vec<StreamEvent>>>>,
}

impl MockProvider {
    fn new(scripts: Vec<Vec<StreamEvent>>) -> Self { ... }
}

impl Provider for MockProvider {
    fn stream_completion<'a>(&'a self, ...) -> BoxStream<'a, ...> {
        let events = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
        Box::pin(stream::iter(events.into_iter().map(Ok)))
    }
}

struct ErrorProvider;
impl Provider for ErrorProvider {
    fn stream_completion<'a>(&'a self, ...) -> BoxStream<'a, ...> {
        Box::pin(stream::iter(vec![Err(ProviderError::Aws("...".to_string()))]))
    }
}
```

For `context.rs` tests: copy `MockProvider` and `ErrorProvider` into `#[cfg(test)]` block.
The `MockProvider` needs to return a summary text stream (TurnEnd with token counts included).

## TuiApp::headless() Constructor
**File:** `ap/src/tui/mod.rs:256-315`

Creates a `StubProvider` (inside the function using a local struct), creates a minimal `Conversation`,
and builds `TuiApp` with all fields. 23 existing `TuiApp::headless()` call sites.

`headless_with_limit` follows the same pattern but accepts `context_limit: Option<u32>` and sets
the new `context_limit` field.

## TuiApp::new() Signature
**File:** `ap/src/tui/mod.rs:232-255`

```rust
pub fn new(
    conv: Arc<tokio::sync::Mutex<Conversation>>,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<Middleware>,
    model_name: String,
) -> Result<Self>
```

Adding `context_limit: Option<u32>` to `new()` would break the call in `main.rs:run_tui()`.
Design says: add field to `TuiApp`, wire from `config.context.limit` in `run_tui()`,
don't change `new()` signature OR add it as last param. The call in `run_tui` must be updated.

## render_status_bar
**File:** `ap/src/tui/ui.rs:44-67`

```rust
fn render_status_bar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let text = format!(
        " ap │ {} │ {} │ Msgs: {} │ Tokens: ↑{:.1}k ↓{:.1}k │ Cost: ${:.4}",
        app.model_name, mode_label, app.conversation_messages, input_k, output_k, cost,
    );
    ...
}
```

New `ctx: XX.Xk` section appended. Must read `app.last_input_tokens` and `app.context_limit`.

## handle_ui_event Usage case
**File:** `ap/src/tui/mod.rs:412-415`

```rust
TurnEvent::Usage { input_tokens, output_tokens } => {
    self.total_input_tokens += input_tokens;
    self.total_output_tokens += output_tokens;
}
```

New behavior: also set `self.last_input_tokens = input_tokens` (replaces, doesn't accumulate).

## Provider Trait
**File:** `ap/src/provider/mod.rs:70-78`

`stream_completion` takes `messages: &'a [Message]`, `tools: &'a [serde_json::Value]`,
`system_prompt: Option<&'a str>`. The `summarise_messages` function must call this with:
- A single `User` message with the summarization prompt
- Empty tools list `&[]`
- `None` system prompt

## Message content types
**File:** `ap/src/provider/mod.rs:20-52`

`MessageContent::Text { text }`, `ToolUse { id, name, input }`, `ToolResult { tool_use_id, content, is_error }`.

For `estimate_message_tokens`: each content variant contributes differently to char count:
- `Text { text }` → `text.chars().count()`
- `ToolUse { name, input }` → `name.len() + input.to_string().len()`
- `ToolResult { content }` → `content.len()`

## lib.rs — module registration
**File:** `ap/src/lib.rs`

All modules listed. `context` module must be added: `pub mod context;`

## Cargo.toml — no new dependencies needed
`anyhow` ✓, `futures` ✓, `tokio` ✓ (with full features), `serde` ✓.
No `tiktoken-rs` needed (heuristic chars/4).

## #[tokio::test] usage
Existing tests in `turn.rs` use `#[tokio::test]` for async tests. Same pattern for `context.rs`.
`#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` is standard in test modules.
