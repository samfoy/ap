# Model Switching — Implementation PROMPT

## Vision

`ap` currently fixes the active model at startup from `ap.toml` / the
`--model` CLI flag and never changes it mid-session. The goal is to make model
selection a first-class runtime operation:

- `/model <id>` typed in the TUI input box switches the model immediately —
  the very next turn uses the new model.
- `--model <id>` on the CLI overrides whatever is in config at startup (all
  modes: TUI and headless `--prompt`).
- The active model is always visible in the TUI status bar.
- Model switching works identically whether the active backend is Bedrock or
  an OpenAI-compatible provider (the model string is passed through
  `build_provider` which already handles both).
- A lightweight `~/.ap/models.json` file records the last N models used so
  the TUI can offer quick switching via `/model` with tab-completion hints
  (the models list is maintained automatically; no user action required).

No new dependencies are required. All existing tests must continue to pass.

---

## Technical Requirements

### 1. `--model` CLI flag (`src/main.rs`)

Add to the `Args` struct:

```rust
/// Override the model from config at startup
#[arg(short = 'm', long = "model")]
model: Option<String>,
```

Apply it immediately after `AppConfig::load()`:

```rust
if let Some(m) = args.model {
    config.provider.model = m;
}
```

This must happen **before** `build_provider` and before the `Conversation` is
constructed, so both pick up the overridden model.

---

### 2. Recent-models store (`src/models.rs`, new file)

```rust
/// Persistent list of recently used model IDs, stored at
/// `~/.ap/models.json` as a JSON array of strings.
///
/// The list is capped at `MAX_RECENT` entries (most-recent first).
/// Duplicate IDs are de-duplicated: recording an existing ID moves
/// it to the front rather than appending another copy.
pub struct RecentModels {
    path: PathBuf,
    pub models: Vec<String>,
}

const MAX_RECENT: usize = 10;

impl RecentModels {
    /// Load from `~/.ap/models.json`, or return an empty list if the file
    /// does not exist. Returns `Err` only on a genuine I/O / parse failure.
    pub fn load() -> Result<Self>;

    /// Load from an explicit path (for tests).
    pub fn load_from(path: PathBuf) -> Result<Self>;

    /// Record `model_id` as the most-recently used model.
    /// Moves duplicates to the front; trims to MAX_RECENT.
    /// Does NOT persist — call `save()` afterward.
    pub fn record(&mut self, model_id: impl Into<String>);

    /// Persist `self.models` to `self.path`, creating parent dirs as needed.
    pub fn save(&self) -> Result<()>;
}
```

**Exact type signature** for `load_from`:

```rust
pub fn load_from(path: PathBuf) -> Result<Self> { ... }
```

The stored format is a plain JSON array, e.g.:

```json
["us.anthropic.claude-sonnet-4-6", "gpt-4o", "llama3"]
```

Use `serde_json` for serialisation (already in `Cargo.toml`). Use `dirs::home_dir()`
for the default path (already in `Cargo.toml`).

Expose `RecentModels` from `src/lib.rs` as `pub mod models;`.

---

### 3. `/model <id>` command parsing (`src/tui/mod.rs`)

Inside `TuiApp::handle_submit`, **before** echoing the input to `chat_history`
and spawning a turn task, check for the `/model` command:

```rust
if let Some(new_model) = trimmed.strip_prefix("/model ").map(str::trim) {
    self.handle_model_switch(new_model.to_string()).await;
    return;
}
```

Add a new private method:

```rust
async fn handle_model_switch(&mut self, model_id: String) {
    // 1. Update self.model_name (status bar reflects change immediately).
    // 2. Update conv.model inside the Arc<Mutex<Conversation>>.
    // 3. Record the new model in RecentModels (best-effort, ignore errors).
    // 4. Push a system notice into chat_history:
    //    ChatEntry::AssistantDone(vec![ChatBlock::Text(
    //        format!("\n[Model switched to: {model_id}]\n")
    //    )])
}
```

`/model` with no argument (just `/model`) should push a notice showing the
current model and a hint listing any known recent models:

```
[Current model: us.anthropic.claude-sonnet-4-6]
[Recent: gpt-4o, llama3]   ← only shown when ~/.ap/models.json has entries
```

---

### 4. `Conversation::with_model` builder (`src/types.rs`)

Add an immutable builder to `Conversation`:

```rust
/// Return a new `Conversation` with the model field replaced.
/// All other fields (id, messages, config, system_prompt) are preserved.
pub fn with_model(mut self, model: impl Into<String>) -> Self {
    self.model = model.into();
    self
}
```

This is the pure-FP equivalent of mutating `conv.model` directly — the
spawned turn task uses `conv_arc.lock().await.clone().with_model(...)` when
it needs the model field to match the one currently selected in the TUI.

---

### 5. Status bar: always show active model (`src/tui/ui.rs`)

The status bar already renders `app.model_name`. No structural change is
needed — `model_name` is kept in sync by `handle_model_switch`. The
**display format** must make the model visually prominent:

```
 ap │ us.anthropic.claude-sonnet-4-6 │ INSERT │ Msgs: 3 │ …
```

The `model_name` field is the source of truth for what the status bar
renders. Verify (via unit test) that after a model switch event the status
bar string contains the new model name.

---

### 6. Record model on turn completion + headless startup

In `run_headless` (`src/main.rs`), after a successful turn, record the model
in `RecentModels` (best-effort: log a warning on failure, never exit):

```rust
// After turn succeeds:
if let Ok(mut recent) = ap::models::RecentModels::load() {
    recent.record(&config.provider.model);
    if let Err(e) = recent.save() {
        eprintln!("ap: warning: could not update recent models: {e}");
    }
}
```

In `run_tui`, record the initial model at startup (same pattern).

---

### 7. Provider model field must be respected at turn time

`BedrockProvider` and `OpenAiCompatProvider` both embed the model string at
construction time. When the model is switched mid-session, `conv.model`
changes but the `provider` Arc still uses the old model string.

**Solution**: `build_provider` is **not** called again on a model switch.
Instead, the `Provider` trait gains a method:

```rust
pub trait Provider: Send + Sync {
    fn stream_completion<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [serde_json::Value],
        system_prompt: Option<&'a str>,
        model_override: Option<&'a str>,  // NEW — None means use provider's default
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>>;
}
```

When `model_override` is `Some(id)`, the provider substitutes that model ID
into its API request instead of its own `self.model`. When it is `None`, the
existing behaviour is preserved.

**`turn()` signature change**:

```rust
pub async fn turn(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
) -> Result<(Conversation, Vec<TurnEvent>)>
```

The signature is **unchanged**. Internally, `turn_loop` passes
`Some(conv.model.as_str())` as `model_override` to `provider.stream_completion`.
This means `conv.model` always wins, regardless of what model the provider
was constructed with. It also means the Bedrock and OpenAI-compat providers
both need updating.

Update every call-site of `stream_completion` inside `src/turn.rs`,
`src/context.rs`, and any test mock providers:

```rust
// In turn_loop:
let mut stream = provider.stream_completion(
    &messages_snapshot,
    &tool_schemas,
    system_prompt,
    Some(conv.model.as_str()),  // always pass model from conversation
);
```

Update `BedrockProvider::stream_completion` to substitute `model_override`
for `self.model` in the `invoke_model_with_response_stream` call.

Update `OpenAiCompatProvider::stream_completion` similarly for the `"model"`
field of the JSON request body.

Update every `impl Provider` in test code (mock providers in `src/turn.rs`,
`src/context.rs`, `src/tui/mod.rs`) to accept the new parameter — they can
ignore it.

---

## Ordered Implementation Steps

Each step must leave `cargo build` **and** `cargo test --lib` green before
moving to the next.

---

### Step 1 — `Conversation::with_model` + tests

**Files:** `src/types.rs`

Add `pub fn with_model(mut self, model: impl Into<String>) -> Self` to
`Conversation`.

**New tests** (add to the existing `#[cfg(test)]` block in `src/types.rs`):

```rust
#[test]
fn conversation_with_model_replaces_field() {
    let conv = Conversation::new("id", "old-model", AppConfig::default());
    let conv2 = conv.with_model("new-model");
    assert_eq!(conv2.model, "new-model");
}

#[test]
fn conversation_with_model_preserves_messages() {
    let conv = Conversation::new("id", "old", AppConfig::default())
        .with_user_message("hello")
        .with_model("new");
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.model, "new");
}

#[test]
fn conversation_with_model_preserves_id_and_config() {
    let conv = Conversation::new("my-id", "old", AppConfig::default())
        .with_model("new");
    assert_eq!(conv.id, "my-id");
}
```

`cargo test --lib` must pass.

---

### Step 2 — `--model` CLI flag

**Files:** `src/main.rs`

1. Add `model: Option<String>` to the `Args` struct with the correct
   `#[arg(short = 'm', long = "model")]` attribute.
2. After `AppConfig::load()` (and the existing `context_limit` override),
   apply the model override:
   ```rust
   if let Some(m) = args.model {
       config.provider.model = m;
   }
   ```

**New test** (add to the `#[cfg(test)]` block at the bottom of `src/main.rs`):

```rust
#[test]
fn cli_model_flag_parses() {
    use clap::Parser;
    let args = Args::try_parse_from(["ap", "--model", "gpt-4o"])
        .expect("should parse --model flag");
    assert_eq!(args.model, Some("gpt-4o".to_string()));
}

#[test]
fn cli_model_flag_absent_is_none() {
    use clap::Parser;
    let args = Args::try_parse_from(["ap"]).expect("should parse empty args");
    assert_eq!(args.model, None);
}
```

`cargo test --lib` must pass.

---

### Step 3 — `RecentModels` store

**Files:** `src/models.rs` (new), `src/lib.rs`

Implement `RecentModels` exactly as specified in the Technical Requirements
section above.

Expose from `src/lib.rs`:

```rust
pub mod models;
```

**New tests** (add a `#[cfg(test)]` block in `src/models.rs`):

```rust
#[test]
fn record_adds_to_front() {
    // load_from a nonexistent file, record "a", record "b" → models == ["b", "a"]
}

#[test]
fn record_deduplicates_and_moves_to_front() {
    // load_from a nonexistent file, record "a", record "b", record "a"
    // → models == ["a", "b"]
}

#[test]
fn record_caps_at_max_recent() {
    // Record 12 distinct models; models.len() == MAX_RECENT (10).
}

#[test]
fn save_and_reload_roundtrip() {
    // save to a tempdir path, load_from same path → same models list.
}

#[test]
fn load_nonexistent_returns_empty() {
    // load_from a path that doesn't exist → Ok(RecentModels { models: [] }).
}

#[test]
fn load_creates_parent_dirs_on_save() {
    // Point at a nested path that doesn't exist yet, save → file is created.
}
```

`cargo test --lib` must pass.

---

### Step 4 — `Provider` trait: add `model_override` parameter

**Files:** `src/provider/mod.rs`, `src/provider/bedrock.rs`,
`src/provider/openai.rs`, `src/turn.rs`, `src/context.rs`, `src/tui/mod.rs`

This is the largest mechanical step. Every `impl Provider` and every
`stream_completion` call-site must be updated.

#### `src/provider/mod.rs`

Change the trait:

```rust
pub trait Provider: Send + Sync {
    fn stream_completion<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [serde_json::Value],
        system_prompt: Option<&'a str>,
        model_override: Option<&'a str>,
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>>;
}
```

#### `src/provider/bedrock.rs`

In `stream_completion`, resolve the model to use:

```rust
let model = model_override
    .map(str::to_owned)
    .unwrap_or_else(|| self.model.clone());
```

Use `model` (not `self.model`) in the `.model_id(...)` call.

#### `src/provider/openai.rs`

Same pattern: resolve model from `model_override` or `self.model`, use in
the `"model"` JSON field of the request body.

#### `src/turn.rs`

In `turn_loop`, pass `Some(conv.model.as_str())` as `model_override`:

```rust
let mut stream = provider.stream_completion(
    &messages_snapshot,
    &tool_schemas,
    system_prompt,
    Some(conv.model.as_str()),
);
```

#### `src/context.rs`

Find the `stream_completion` call(s) inside the context compression logic and
add `None` (the summariser always uses the provider's default model — no
model override during summarisation):

```rust
provider.stream_completion(&messages, &[], system_prompt, None)
```

#### `src/tui/mod.rs` — `StubProvider` in `headless()`

The stub `impl Provider` used in headless tests needs the new signature:

```rust
fn stream_completion<'a>(
    &'a self,
    _messages: &'a [crate::provider::Message],
    _tools: &'a [serde_json::Value],
    _system_prompt: Option<&'a str>,
    _model_override: Option<&'a str>,
) -> futures::stream::BoxStream<'a, ...> {
    Box::pin(futures::stream::empty())
}
```

#### `src/turn.rs` — `MockProvider` in tests

Same mechanical update: add `_model_override: Option<&'a str>` parameter.

**New test** (add to `src/provider/bedrock.rs` tests):

```rust
#[test]
fn model_override_substitutes_in_request_body() {
    // build_request_body is private, but we can test the logic via
    // BedrockProvider::build_request_body indirectly. Instead, add a
    // helper test that verifies the model string used is the override:
    // Construct a provider with model "model-a", call stream_completion
    // with model_override = Some("model-b"), verify the body sent to
    // Bedrock uses "model-b".
    //
    // Because we cannot intercept the AWS SDK call without mocking, this
    // test instead verifies the *resolution logic* using a public helper:
    // if model_override is Some, it wins; if None, self.model is used.
    //
    // We'll test this by extracting the resolution into a standalone pure fn:
    //   fn resolve_model<'a>(own: &'a str, override_: Option<&'a str>) -> &'a str {
    //       override_.unwrap_or(own)
    //   }
    // and testing that directly.
    let own = "model-a";
    let resolved_with_override = model_override.unwrap_or(own);
    assert_eq!(resolved_with_override, "model-b");
}
```

In practice, expose a `pub(crate) fn resolve_model<'a>(own: &'a str, override_: Option<&'a str>) -> &'a str`
in `src/provider/bedrock.rs` and test it:

```rust
#[test]
fn resolve_model_prefers_override() {
    assert_eq!(resolve_model("default", Some("custom")), "custom");
}

#[test]
fn resolve_model_falls_back_to_own() {
    assert_eq!(resolve_model("default", None), "default");
}
```

`cargo test --lib` must pass with zero compile errors.

---

### Step 5 — TUI `/model` command + `handle_model_switch`

**Files:** `src/tui/mod.rs`, `src/tui/events.rs`

#### `src/tui/mod.rs`

1. Add `handle_model_switch` as a private `async fn` on `TuiApp` exactly as
   specified in Technical Requirements §3.

2. In `handle_submit`, before the existing `/help` check, add:

   ```rust
   // /model with argument — switch immediately
   if let Some(new_model) = trimmed.strip_prefix("/model ").map(str::trim) {
       self.handle_model_switch(new_model.to_string()).await;
       return;
   }
   // /model with no argument — show current + recent
   if trimmed == "/model" {
       self.handle_model_query().await;
       return;
   }
   ```

3. Add `handle_model_query` (shows current model + recent list from file,
   best-effort — if the file cannot be read, just show current model).

#### `src/tui/events.rs`

No changes needed — `/model` is handled entirely in `handle_submit` before
key events reach `handle_key_event`.

**New unit tests** (add to `src/tui/mod.rs` `#[cfg(test)]`):

```rust
#[tokio::test]
async fn handle_model_switch_updates_model_name() {
    let mut app = TuiApp::headless();
    assert_eq!(app.model_name, "test-model");
    app.handle_model_switch("gpt-4o".to_string()).await;
    assert_eq!(app.model_name, "gpt-4o");
}

#[tokio::test]
async fn handle_model_switch_updates_conversation_model() {
    let mut app = TuiApp::headless();
    app.handle_model_switch("new-model".to_string()).await;
    let conv = app.conv.lock().await;
    assert_eq!(conv.model, "new-model");
}

#[tokio::test]
async fn handle_model_switch_pushes_notice_to_chat() {
    let mut app = TuiApp::headless();
    app.handle_model_switch("llama3".to_string()).await;
    // A ChatEntry::AssistantDone with "Model switched to: llama3" must appear
    assert!(app.chat_history.iter().any(|e| match e {
        ChatEntry::AssistantDone(blocks) => blocks.iter().any(|b| match b {
            ChatBlock::Text(t) => t.contains("llama3"),
            _ => false,
        }),
        _ => false,
    }));
}

#[tokio::test]
async fn submit_model_command_does_not_echo_as_user_message() {
    let mut app = TuiApp::headless();
    // Simulate the full submit path
    app.handle_submit("/model gpt-4o".to_string()).await;
    // Must not appear as a User chat entry
    assert!(!app.chat_history.iter().any(|e| matches!(e, ChatEntry::User(_))));
}
```

`cargo test --lib` must pass.

---

### Step 6 — Record model in headless + TUI startup

**Files:** `src/main.rs`

1. In `run_headless`, after a successful turn (just before the `if exit_code != 0` check):

   ```rust
   if exit_code == 0 {
       if let Ok(mut recent) = ap::models::RecentModels::load() {
           recent.record(&config.provider.model);
           if let Err(e) = recent.save() {
               eprintln!("ap: warning: could not update recent models: {e}");
           }
       }
   }
   ```

2. In `run_tui`, after building `initial_conv` and before calling `app.run()`,
   record the startup model:

   ```rust
   if let Ok(mut recent) = ap::models::RecentModels::load() {
       recent.record(&config.provider.model);
       if let Err(e) = recent.save() {
           eprintln!("ap: warning: could not update recent models: {e}");
       }
   }
   ```

No new unit tests for this step — the `RecentModels` API is already covered
in Step 3. `cargo build` and `cargo test --lib` must remain green.

---

### Step 7 — Status bar model visibility test + `ap.toml.example`

**Files:** `src/tui/ui.rs`, `ap.toml.example`

1. Add a unit test to `src/tui/ui.rs` verifying the status bar string contains
   the model name:

   ```rust
   #[test]
   fn status_bar_contains_model_name() {
       // format_ctx_segment is already tested; here we test the full status bar
       // text by inspecting TuiApp state after a model switch.
       // Since render() requires a real Frame, we test via the field values
       // that feed into it:
       let mut app = TuiApp::headless();
       assert!(app.model_name.contains("test-model"),
           "default headless model should be test-model");
       // After a switch the field is updated (covered by tui/mod.rs tests).
       // This test just verifies the status bar reads from model_name.
       app.model_name = "claude-opus-4".to_string();
       assert_eq!(app.model_name, "claude-opus-4");
   }
   ```

2. Update `ap.toml.example` to document the `--model` flag and the
   `/model <id>` command in a comment block near the `[provider]` section:

   ```toml
   # Model can also be overridden at startup with: ap --model <id>
   # Or switched mid-session in the TUI with: /model <id>
   # Recent models are remembered in ~/.ap/models.json
   ```

`cargo test --lib` must pass.

---

## Acceptance Criteria

All of the following must be true for the loop to be considered complete:

1. **`cargo build` is clean** — zero errors, zero warnings (the project's
   existing `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`
   denials must continue to pass).

2. **`cargo test --lib` passes** — all pre-existing tests continue to pass;
   all new tests added in Steps 1–7 pass.

3. **`Conversation::with_model`** exists, returns a new `Conversation` with
   the model field replaced, and preserves all other fields.

4. **`--model` CLI flag** is accepted by `clap`, is `Option<String>`, and when
   supplied overrides `config.provider.model` before `build_provider` is called
   in both `run_headless` and `run_tui`.

5. **`RecentModels`** exists in `src/models.rs`, exposed as `ap::models`:
   - `load()` returns an empty list when `~/.ap/models.json` is absent.
   - `load_from(path)` is the testable variant used in all unit tests.
   - `record(id)` de-duplicates and moves duplicates to the front.
   - The list is capped at `MAX_RECENT` (10).
   - `save()` creates parent directories automatically.
   - Round-trip through `save()` + `load_from()` returns the same list.

6. **`Provider::stream_completion`** signature includes
   `model_override: Option<&'a str>` as the final parameter. Every `impl
   Provider` in the codebase accepts this parameter. `BedrockProvider` and
   `OpenAiCompatProvider` substitute `model_override` for `self.model` when
   it is `Some`.

7. **`turn_loop`** passes `Some(conv.model.as_str())` as `model_override` to
   `provider.stream_completion`, ensuring `conv.model` always determines which
   model is called.

8. **`/model <id>`** typed in the TUI input box:
   - Does **not** appear as a `ChatEntry::User` entry.
   - Updates `app.model_name` immediately.
   - Updates `conv.model` inside the `Arc<Mutex<Conversation>>`.
   - Pushes a `ChatEntry::AssistantDone` notice containing the new model id.
   - Records the new model in `RecentModels` (best-effort).

9. **`/model` with no argument** pushes a notice showing the current model
   name. If `~/.ap/models.json` has entries, the notice also lists them.

10. **`app.model_name`** in the TUI is always the active model — it is set at
    startup to `config.provider.model` and updated by every call to
    `handle_model_switch`.

11. **`ap.toml.example`** contains a comment documenting `--model` and
    `/model <id>`.

12. **`src/context.rs`** passes `None` for `model_override` so the
    summarisation step always uses the provider's built-in model rather than
    the conversation's current model.

13. **No new `Cargo.toml` dependencies** are required — all functionality is
    implemented with crates already present.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project
builds clean.
