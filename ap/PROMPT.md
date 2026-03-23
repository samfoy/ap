# PROMPT.md — Model Switching

## Vision

Users should be able to swap the active LLM model at any time without
restarting `ap`. The active model is always visible in the TUI status bar.
Switching is instant: the next turn uses the new model. A persistent
`~/.ap/models.json` file remembers recently used models so the user can
quickly return to a favourite.

The feature touches four layers:
1. **Config** — `--model` CLI flag overrides `config.toml` at startup.
2. **Model registry** — `~/.ap/models.json` stores a recency-ordered list;
   pure load/save functions, no global state.
3. **Runtime switching** — `/model <id>` slash-command in the TUI input box
   switches the provider mid-session.
4. **TUI rendering** — status bar always shows the active model ID; the
   command is documented in the help overlay.

Provider-agnostic: the `BedrockProvider` already accepts `model` at
construction. Switching means constructing a new provider instance.
OpenAI-compat providers (future) follow the same pattern.

---

## Functional Requirements

### R1 — `--model` CLI flag
`ap --model <id>` sets `config.provider.model` before any session logic runs.
The value is validated to be a non-empty string only; no whitelist check.

### R2 — `RecentModels` store
- File: `~/.ap/models.json`
- Format: `{ "models": ["id1", "id2", ...] }` — ordered most-recent first.
- Max entries: 10 (oldest dropped).
- Operations: `load() -> Result<RecentModels>`, `save(&self) -> Result<()>`,
  `push(id: &str) -> RecentModels` (pure, returns new value).
- Saving happens after every successful model switch.

### R3 — `/model <id>` slash-command
- Parsed in `events::handle_key_event` on Enter, before submitting to the
  turn pipeline. If the input buffer starts with `/model ` (note the space),
  it is intercepted and routed as `Action::ModelSwitch(id)`.
- `/model` with no argument (or only whitespace after) → show current model
  in the chat history as a system notice; no switch occurs.
- The `/model` command must NOT be forwarded to the LLM.

### R4 — TUI model display
- `TuiApp::model_name: String` already exists and is shown in the status bar.
- On `Action::ModelSwitch`, `model_name` is updated immediately (synchronously,
  before the next turn).

### R5 — Provider hot-swap
- `TuiApp` holds `provider: Arc<dyn Provider>`. On `/model <id>`:
  1. Construct a new `BedrockProvider` (same region, new model id).
  2. Replace `self.provider` with `Arc::new(new_provider)`.
  3. Update `self.conv` (under the mutex) to set `conv.model = new_id`.
  4. Append a system notice to `chat_history`.
  5. Call `RecentModels::push` and save.
- If construction fails, show an error notice and leave the current provider
  unchanged.

### R6 — Headless `--model` flag
In headless (`-p`) mode the model flag simply overrides `config.provider.model`
before the provider is constructed. No runtime switching needed.

---

## Precise Rust Types and Signatures

### New file: `src/models.rs`

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_RECENT: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentModels {
    pub models: Vec<String>,
}

impl Default for RecentModels {
    fn default() -> Self { Self { models: Vec::new() } }
}

impl RecentModels {
    /// Pure: return new `RecentModels` with `id` prepended, capped at MAX_RECENT.
    /// Deduplicates: if `id` already present, move it to front.
    pub fn push(&self, id: &str) -> Self { ... }

    /// Load from `~/.ap/models.json`. Returns `Default::default()` if the file
    /// does not exist.
    pub fn load() -> Result<Self> { ... }

    /// Testable variant — loads from an explicit path.
    pub fn load_from(path: &std::path::Path) -> Result<Self> { ... }

    /// Save to `~/.ap/models.json`.
    pub fn save(&self) -> Result<()> { ... }

    /// Testable variant — saves to an explicit path.
    pub fn save_to(&self, path: &std::path::Path) -> Result<()> { ... }
}
```

### Changes to `src/main.rs`

```rust
// Add to Args:
/// Override the model from config (e.g. us.anthropic.claude-haiku-4-5)
#[arg(long = "model", short = 'm')]
model: Option<String>,

// In main(), after AppConfig::load():
if let Some(m) = args.model {
    config.provider.model = m;
}
```

### Changes to `src/tui/events.rs`

```rust
pub enum Action {
    None,
    Submit(String),
    Quit,
    Cancel,
    ModelSwitch(String),   // NEW: /model <id> parsed here
}
```

`handle_key_event` intercepts Enter when the buffer starts with `/model `:
- Strips the buffer, returns `Action::ModelSwitch(id.trim().to_string())`.
- If the trimmed id is empty, returns `Action::ModelSwitch(String::new())`
  (empty string signals "show current model only").

### Changes to `src/tui/mod.rs`

```rust
// New async method on TuiApp:
async fn handle_model_switch(&mut self, new_id: String);
```

Signature detail:
```rust
async fn handle_model_switch(&mut self, new_id: String) {
    if new_id.is_empty() {
        // Show current model as notice
        self.chat_history.push(ChatEntry::AssistantDone(vec![
            ChatBlock::Text(format!("\n[model: {}]\n", self.model_name))
        ]));
        return;
    }
    match BedrockProvider::new(new_id.clone(), region).await {
        Ok(p) => {
            self.provider = Arc::new(p) as Arc<dyn Provider>;
            self.model_name = new_id.clone();
            self.conv.lock().await.model = new_id.clone();
            // save to recent models
            ...
            self.chat_history.push(/* "Switched to <id>" notice */);
        }
        Err(e) => {
            self.chat_history.push(/* error notice */);
        }
    }
}
```

The region is read from `self.conv.lock().await.config.provider.region`.

### Changes to `src/tui/mod.rs` — event loop wiring

In `event_loop`, the `Action::ModelSwitch` arm:
```rust
events::Action::ModelSwitch(id) => {
    self.handle_model_switch(id).await;
}
```

### Changes to `src/lib.rs`

```rust
pub mod models;    // NEW
```

---

## Ordered Implementation Steps

Each step must compile and pass `cargo test` cleanly before proceeding.

---

### Step 1 — `RecentModels` store (`src/models.rs`)

**Goal:** pure data type + file I/O, fully tested, no TUI changes.

Create `src/models.rs`:

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_RECENT: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RecentModels {
    pub models: Vec<String>,
}

impl RecentModels {
    pub fn push(&self, id: &str) -> Self { ... }
    pub fn load() -> Result<Self> { ... }
    pub fn load_from(path: &Path) -> Result<Self> { ... }
    pub fn save(&self) -> Result<()> { ... }
    pub fn save_to(&self, path: &Path) -> Result<()> { ... }
}
```

Add `pub mod models;` to `src/lib.rs`.

**Tests to write in `src/models.rs`:**

- `push_adds_to_front` — `default().push("m1").models[0] == "m1"`
- `push_deduplicates` — pushing an existing id moves it to front, no
  duplicate remains
- `push_caps_at_max_recent` — after 11 pushes, `models.len() == MAX_RECENT`
- `push_is_pure` — original unchanged after `push`
- `load_from_returns_default_when_missing` — non-existent path → `Default`
- `load_from_parses_valid_json` — write valid JSON, load, verify models vec
- `save_to_then_load_from_roundtrip` — push 3 ids, save to tempfile, load,
  verify order preserved
- `save_to_creates_parent_dirs` — path inside a non-existent subdir is created

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 2 — `--model` CLI flag

**Goal:** `--model` flag overrides `config.provider.model` at startup.

Changes:
1. Add `model: Option<String>` field to `Args` struct in `src/main.rs` with
   `#[arg(long = "model", short = 'm')]`.
2. After `AppConfig::load()`, apply override:
   ```rust
   if let Some(m) = args.model {
       config.provider.model = m;
   }
   ```
3. This must propagate to both `run_headless` and `run_tui` since they both
   receive `config` by value.

**Tests to add in `src/main.rs` `tests` module:**

- `model_flag_present_in_args` — parse `["ap", "--model", "my-model"]` via
  `Args::try_parse_from`, assert `args.model == Some("my-model")`
- `model_short_flag` — parse `["ap", "-m", "x"]`, assert `args.model == Some("x")`
- `model_flag_absent` — parse `["ap"]`, assert `args.model == None`

Use `clap::Parser::try_parse_from` (not `parse()`) so tests don't exit on
failure.

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 3 — `Action::ModelSwitch` in event handler

**Goal:** `/model <id>` input is intercepted at the keyboard layer and never
reaches the LLM.

Changes to `src/tui/events.rs`:

1. Add `ModelSwitch(String)` variant to `Action`.
2. In `handle_key_event`, on `KeyCode::Enter` when buffer is non-empty and not
   waiting, check `input_buffer.trim_start().starts_with("/model")` before the
   normal submit path:
   ```rust
   let text = app.input_buffer.trim().to_string();
   if text.starts_with("/model") {
       let id = text
           .strip_prefix("/model")
           .unwrap_or("")
           .trim()
           .to_string();
       app.input_buffer.clear();
       return Action::ModelSwitch(id);
   }
   ```
3. Add `ModelSwitch` arm to the match in `event_loop` in `src/tui/mod.rs`
   (for now: just call a no-op stub `self.handle_model_switch(id).await`).
   The stub appends a placeholder chat notice so the arm compiles and is
   exercised. Full implementation is Step 4.

**Tests to add in `src/tui/events.rs` `tests` module:**

- `slash_model_with_id_returns_model_switch` — buffer `/model claude-3-haiku`,
  Enter → `Action::ModelSwitch("claude-3-haiku")`
- `slash_model_no_arg_returns_model_switch_empty` — buffer `/model`, Enter →
  `Action::ModelSwitch("")`
- `slash_model_with_spaces_trims_id` — buffer `/model   my-id  `, Enter →
  `Action::ModelSwitch("my-id")`
- `slash_model_clears_input_buffer` — after dispatch, `app.input_buffer` is
  empty
- `regular_message_not_intercepted` — buffer `hello`, Enter →
  `Action::Submit("hello")` (regression guard)
- `slash_model_while_waiting_returns_none` — `is_waiting = true`, Enter →
  `Action::None` (command ignored while turn in progress)

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 4 — `handle_model_switch` on `TuiApp`

**Goal:** full `/model` handling — provider hot-swap, status bar update,
recent models persistence.

Changes to `src/tui/mod.rs`:

1. Add `handle_model_switch` as a real async method (replace the Step 3 stub):

```rust
async fn handle_model_switch(&mut self, new_id: String) {
    if new_id.is_empty() {
        self.chat_history.push(ChatEntry::AssistantDone(vec![
            ChatBlock::Text(format!("\n[model: {}]\n", self.model_name))
        ]));
        return;
    }
    let region = {
        let conv = self.conv.lock().await;
        conv.config.provider.region.clone()
    };
    match BedrockProvider::new(new_id.clone(), region).await {
        Ok(p) => {
            self.provider = Arc::new(p) as Arc<dyn Provider>;
            self.model_name = new_id.clone();
            {
                let mut conv = self.conv.lock().await;
                conv.model = new_id.clone();
            }
            // Push to recent models (best-effort; ignore save errors)
            if let Ok(recent) = crate::models::RecentModels::load() {
                let updated = recent.push(&new_id);
                let _ = updated.save();
            }
            self.chat_history.push(ChatEntry::AssistantDone(vec![
                ChatBlock::Text(format!("\n[switched to model: {new_id}]\n"))
            ]));
        }
        Err(e) => {
            self.chat_history.push(ChatEntry::AssistantDone(vec![
                ChatBlock::Text(format!("\n[model switch failed: {e}]\n"))
            ]));
        }
    }
}
```

2. Wire the action in `event_loop`:
```rust
events::Action::ModelSwitch(id) => {
    self.handle_model_switch(id).await;
}
```

3. Update `TuiApp::headless()` and `headless_with_limit()`: these already
   compile since they don't call `handle_model_switch`. No changes needed.

**Tests to add in `src/tui/mod.rs` `tests` module:**

Because `handle_model_switch` calls `BedrockProvider::new` (async AWS SDK,
requires real credentials), unit-test the *observable state changes* through
the public fields, not the provider construction itself. Use a helper that
exercises the no-arg path and the chat-history update path directly.

- `handle_model_switch_empty_shows_current_model` — call
  `app.handle_model_switch("".to_string()).await` on a headless app, verify
  a `ChatEntry::AssistantDone` is appended that contains the current model
  name string.
- `model_name_is_shown_in_chat_notice_on_empty_switch` — same as above,
  check the text block contains `app.model_name`.
- `handle_model_switch_updates_model_name_field` — provide a test double:
  since `BedrockProvider::new` can't be called in unit tests, test the state
  update path in isolation by calling a helper that directly sets `model_name`
  and appends the notice. Alternatively, test via an integration-style check
  on the headless app where the switch fails (AWS creds absent in CI) and
  confirm the error notice is appended and `model_name` is UNCHANGED.
- `action_model_switch_is_handled_in_event_loop` — use the existing
  `handle_key_event` test infrastructure: push `/model foo` into
  `input_buffer`, call `handle_key_event(Enter)`, assert
  `Action::ModelSwitch("foo")` (already covered by Step 3 tests; keep as
  regression).

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 5 — Status bar model display & help text

**Goal:** model name is visibly current in the status bar at all times;
`/model` appears in the help overlay.

The status bar already renders `app.model_name`. Since `handle_model_switch`
updates `self.model_name` synchronously before returning, the next
`terminal.draw()` call will show the new model automatically. No rendering
changes are strictly required, but this step adds:

1. **`format_model_segment` helper** in `src/tui/ui.rs`:

```rust
/// Format the model segment for the status bar.
/// Truncates to 30 chars with `…` if longer.
pub(crate) fn format_model_segment(model: &str) -> String {
    if model.chars().count() > 30 {
        let truncated: String = model.chars().take(29).collect();
        format!("{truncated}…")
    } else {
        model.to_string()
    }
}
```

Use this in `render_status_bar` in place of the raw `app.model_name` string
interpolation, so long model IDs don't break the layout.

2. **Help text**: The help overlay (if `show_help` is used) or the input
   placeholder should mention `/model`. Add a comment to `render_input_line`
   referencing the command. If there is no existing help overlay, add the
   `/model <id>` line as a comment; do not build a new UI component.

**Tests to add in `src/tui/ui.rs` `tests` module:**

- `format_model_segment_short_name_unchanged` — `"claude-3"` → `"claude-3"`
- `format_model_segment_exactly_30_chars_unchanged` — 30-char string →
  returned as-is
- `format_model_segment_31_chars_truncated` — 31-char string → 30 chars + `…`
- `format_model_segment_very_long_name` — 60-char model id → ends with `…`,
  total length ≤ 31 chars
- `status_bar_uses_model_name_from_app` — create a headless app, verify the
  `model_name` field is `"test-model"` (already passing; keep as regression
  guard).

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 6 — Integration: `--model` flag wires into both modes

**Goal:** end-to-end compile + test; `--model` accepted by `ap --help`;
initial `model_name` in TUI reflects the flag.

1. Verify `run_tui` passes `config.provider.model.clone()` as `model_name` to
   `TuiApp::new` — it already does. No change needed if the value flows
   correctly from the overridden config.

2. Verify `run_headless` uses `config.provider.model` when constructing
   `BedrockProvider` — it already does.

3. Add the model name to the headless startup log:
   ```rust
   eprintln!("ap: model: {}", config.provider.model);
   ```
   (only when not resuming a session — i.e. at the top of `run_headless`
   before any session logic).

4. Ensure `RecentModels::load()` and `save()` are exercised at least via the
   `save_to_then_load_from_roundtrip` test from Step 1. No additional
   integration test needed for the flag itself since Step 2 tests parse the
   CLI argument.

**Tests (Step 6 is mostly a compile + wiring verification):**

- `model_name_in_tui_app_matches_config` — construct `TuiApp::new` with
  `model_name = "custom-model"`, assert `app.model_name == "custom-model"`.
  (This already passes from existing tests; confirm it still does.)
- `config_model_override_applied_before_provider_construction` — unit test in
  `main.rs`: parse `["ap", "--model", "override-model"]`, apply the override
  to a default config, assert `config.provider.model == "override-model"`.
  This tests the in-`main` logic without spawning a process.

**Final compile check:**

```
cargo build 2>&1
cargo test 2>&1 | tail -20
cargo clippy -- -D warnings 2>&1 | tail -20
```

All must be clean.

---

## Acceptance Criteria

- [ ] **AC1** — `cargo build` succeeds with zero errors and zero warnings
  (`-D warnings`).
- [ ] **AC2** — `cargo test` passes all tests (no failures, no ignored tests
  added by this feature).
- [ ] **AC3** — `ap --help` output includes `-m, --model <MODEL>`.
- [ ] **AC4** — `ap -m us.anthropic.claude-haiku-4-5 -p "hi"` starts the
  headless turn using the specified model (verifiable via the `"ap: model: …"`
  log line written to stderr).
- [ ] **AC5** — `RecentModels::push` is pure: the original value is unchanged
  and the returned value has the new id at position 0.
- [ ] **AC6** — `RecentModels::push` deduplicates: pushing an existing id does
  not create two entries.
- [ ] **AC7** — `RecentModels::push` caps at `MAX_RECENT` (10) entries.
- [ ] **AC8** — `save_to_then_load_from_roundtrip` passes: IDs survive a
  JSON round-trip in insertion order.
- [ ] **AC9** — `/model <id>` (Enter) produces `Action::ModelSwitch("id")`,
  not `Action::Submit`.
- [ ] **AC10** — `/model` (no arg, Enter) produces `Action::ModelSwitch("")`,
  not `Action::Submit`.
- [ ] **AC11** — `/model` with `is_waiting = true` produces `Action::None`.
- [ ] **AC12** — `handle_model_switch("")` appends a chat notice containing
  the current model name without changing `model_name` or the provider.
- [ ] **AC13** — `format_model_segment` truncates model ids longer than 30
  chars to 29 chars + `…`.
- [ ] **AC14** — After `Action::ModelSwitch`, `app.model_name` reflects the
  new id immediately (synchronously, before the next render).
- [ ] **AC15** — `src/models.rs` is listed under `pub mod models;` in
  `src/lib.rs` and all its tests pass.

---

## Notes for Ralph

- Do not use `unwrap()` or `expect()` in non-test code — the project enforces
  `clippy::unwrap_used = "deny"` and `clippy::expect_used = "deny"`.
- Do not add a `rand` crate dependency — use UUID bytes for entropy as the
  existing `generate_name()` does.
- `BedrockProvider::new` is `async` and returns `anyhow::Result<Self>`. In
  `handle_model_switch`, wrap the call in an `await` and match the result.
- `TuiApp.conv` is `Arc<tokio::sync::Mutex<Conversation>>`. Lock it with
  `.lock().await` and drop the guard before any `.await` point that might
  contend on the same mutex.
- The `models.json` file path is `dirs::home_dir()?.join(".ap/models.json")`.
  Use `dirs::home_dir()` (already in `Cargo.toml`) and return a descriptive
  `anyhow::Context` error if the home directory cannot be determined.
- Each step must independently compile. Test each step with `cargo test -q`
  before moving to the next.
- Write all tests inside the same file as the code they test (inline
  `#[cfg(test)] mod tests { ... }` block), matching the existing project
  convention.
- For Step 4 unit tests: `BedrockProvider::new` will succeed in construction
  (no credential check) but fail at call time in CI. The safe test strategy
  is to verify the empty-id path (no provider construction) and the
  chat-history side effects of the error path.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project
builds clean.
