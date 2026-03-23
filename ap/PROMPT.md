# PROMPT.md — Model Switching

## Vision

Users should be able to swap the active LLM model at any time without
restarting `ap`. The active model is always visible in the TUI status bar.
Switching is instant: the very next turn uses the new model. A persistent
`~/.ap/models.json` file remembers recently used models so the user can
quickly cycle back to a favourite.

The feature touches four layers:
1. **Config** — `--model` / `-m` CLI flag overrides `config.provider.model`
   at startup, before any session or provider logic runs.
2. **Model registry** — `~/.ap/models.json` stores a recency-ordered list;
   pure load/save functions, no global state.
3. **Runtime switching** — `/model <id>` slash-command in the TUI input box
   intercepts Enter, hot-swaps the provider, and updates the conversation's
   `model` field — the command is never forwarded to the LLM.
4. **TUI rendering** — the status bar already shows `app.model_name`; a new
   helper truncates long IDs so the layout never breaks.

Provider-agnostic: `BedrockProvider` already accepts `model` at construction
time. Hot-swapping means constructing a fresh provider instance with the new
model id and the same region. Future OpenAI-compat providers follow the same
`Provider` trait pattern.

---

## Technical Requirements

### R1 — `--model` CLI flag

`ap --model <id>` (short: `-m`) sets `config.provider.model` before any
session logic runs. Applied in `main()` after `AppConfig::load()`:

```rust
if let Some(m) = args.model {
    config.provider.model = m;
}
```

Both `run_headless` and `run_tui` receive `config` by value after this
override; no further changes to those functions are required.

### R2 — `RecentModels` store (`src/models.rs`)

New public module. Pure data operations; file I/O isolated to two functions.

```rust
pub const MAX_RECENT: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RecentModels {
    pub models: Vec<String>,   // ordered most-recent first
}

impl RecentModels {
    /// Pure: return new value with `id` at index 0, deduplicated, capped at MAX_RECENT.
    pub fn push(&self, id: &str) -> RecentModels;

    /// Load from `~/.ap/models.json`; return Default if missing.
    pub fn load() -> Result<RecentModels>;

    /// Testable: load from an explicit path; return Default if missing.
    pub fn load_from(path: &Path) -> Result<RecentModels>;

    /// Save to `~/.ap/models.json`.
    pub fn save(&self) -> Result<()>;

    /// Testable: save to an explicit path (creates parent dirs).
    pub fn save_to(&self, path: &Path) -> Result<()>;
}
```

JSON format on disk:

```json
{ "models": ["us.anthropic.claude-sonnet-4-6", "us.anthropic.claude-haiku-4-5"] }
```

### R3 — `Action::ModelSwitch` (events layer)

New variant added to `src/tui/events.rs`:

```rust
pub enum Action {
    None,
    Submit(String),
    Quit,
    Cancel,
    ModelSwitch(String),   // new — id is "" when no arg given
}
```

`handle_key_event` intercepts `KeyCode::Enter` (when `!is_waiting` and buffer
non-empty) **before** the normal submit path:

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
// ... existing submit logic
```

`/model` with `is_waiting = true` → `Action::None` (same guard as Submit).

### R4 — `handle_model_switch` on `TuiApp` (`src/tui/mod.rs`)

New `async fn` on `TuiApp`:

```rust
async fn handle_model_switch(&mut self, new_id: String) {
    if new_id.is_empty() {
        // Show current model — no provider change
        self.chat_history.push(ChatEntry::AssistantDone(vec![
            ChatBlock::Text(format!("\n[model: {}]\n", self.model_name))
        ]));
        return;
    }
    let region = {
        let conv = self.conv.lock().await;
        conv.config.provider.region.clone()
    };  // guard dropped before next await
    match BedrockProvider::new(new_id.clone(), region).await {
        Ok(p) => {
            self.provider = Arc::new(p) as Arc<dyn Provider>;
            self.model_name = new_id.clone();
            {
                let mut conv = self.conv.lock().await;
                conv.model = new_id.clone();
            }  // guard dropped
            if let Ok(recent) = crate::models::RecentModels::load() {
                let _ = recent.push(&new_id).save();   // best-effort
            }
            self.chat_history.push(ChatEntry::AssistantDone(vec![
                ChatBlock::Text(format!("\n[switched to model: {new_id}]\n"))
            ]));
        }
        Err(e) => {
            self.chat_history.push(ChatEntry::AssistantDone(vec![
                ChatBlock::Text(format!("\n[model switch failed: {e}]\n"))
            ]));
            // provider, model_name, conv.model all UNCHANGED
        }
    }
}
```

Wired into `event_loop`:

```rust
events::Action::ModelSwitch(id) => {
    self.handle_model_switch(id).await;
}
```

### R5 — `format_model_segment` helper (`src/tui/ui.rs`)

```rust
/// Truncate a model id to at most 30 chars for the status bar.
pub(crate) fn format_model_segment(model: &str) -> String {
    if model.chars().count() > 30 {
        let truncated: String = model.chars().take(29).collect();
        format!("{truncated}…")
    } else {
        model.to_string()
    }
}
```

`render_status_bar` uses `format_model_segment(&app.model_name)` in the
format string instead of `&app.model_name` directly.

### R6 — `src/lib.rs` export

```rust
pub mod models;   // new line
```

---

## Ordered Implementation Steps

Each step must independently compile (`cargo build`) and pass `cargo test -q`
before the next step begins.

---

### Step 1 — `RecentModels` store (`src/models.rs`)

**Goal:** pure data type + file I/O, zero TUI or CLI changes.

**What to create:**

`src/models.rs` containing `RecentModels` exactly as specified in R2.

Key implementation notes:
- `push`: collect existing entries, remove any that equal `id`, prepend `id`,
  truncate to `MAX_RECENT`. Return `Self { models: ... }`.
- `load_from`: if path does not exist, return `Ok(Self::default())`. On parse
  error, return `Err`.
- `save_to`: create parent directories with `std::fs::create_dir_all`. Write
  pretty JSON.
- `load` / `save`: delegate to `load_from` / `save_to` with
  `dirs::home_dir()?.join(".ap/models.json")`.

Add `pub mod models;` to `src/lib.rs`.

**Tests (inline `#[cfg(test)]` block in `src/models.rs`):**

| Test name | What it asserts |
|-----------|----------------|
| `push_adds_to_front` | `RecentModels::default().push("m1").models[0] == "m1"` |
| `push_deduplicates` | push "m1" twice → `models.len() == 1`, `models[0] == "m1"` |
| `push_deduplicates_moves_to_front` | `[m1, m2].push("m2")` → `[m2, m1]` |
| `push_caps_at_max_recent` | push 11 distinct ids → `models.len() == MAX_RECENT` |
| `push_oldest_dropped_when_at_cap` | after 11 pushes the first id pushed is absent |
| `push_is_pure` | original `RecentModels` unchanged after calling `push` |
| `load_from_returns_default_when_missing` | non-existent path → `Ok(Default::default())` |
| `load_from_parses_valid_json` | write `{"models":["a","b"]}` → `models == ["a","b"]` |
| `load_from_returns_err_on_invalid_json` | malformed JSON → `Err` |
| `save_to_then_load_from_roundtrip` | push 3 ids, `save_to`, `load_from` → same order |
| `save_to_creates_parent_dirs` | save to `<tempdir>/sub/models.json` — no pre-created subdir |

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 2 — `--model` / `-m` CLI flag

**Goal:** the flag is parsed; it overrides `config.provider.model` before any
provider construction.

**Changes to `src/main.rs`:**

1. Add to `Args`:
   ```rust
   /// Override the active model (e.g. us.anthropic.claude-haiku-4-5)
   #[arg(long = "model", short = 'm')]
   model: Option<String>,
   ```

2. In `main()`, immediately after `AppConfig::load()`:
   ```rust
   if let Some(m) = args.model {
       config.provider.model = m;
   }
   ```

No changes to `run_headless` or `run_tui` — they already receive `config` by
value after the override is applied.

**Tests (add to existing `#[cfg(test)] mod tests` in `src/main.rs`):**

Use `clap::Parser::try_parse_from` so failures don't call `process::exit`.

| Test name | Assertion |
|-----------|-----------|
| `model_long_flag_parsed` | `Args::try_parse_from(["ap","--model","x-model"])` → `args.model == Some("x-model")` |
| `model_short_flag_parsed` | `Args::try_parse_from(["ap","-m","x"])` → `args.model == Some("x")` |
| `model_flag_absent` | `Args::try_parse_from(["ap"])` → `args.model == None` |
| `model_flag_overrides_config` | build a default `AppConfig`, set `config.provider.model` via the override pattern, assert the field equals the override value |

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 3 — `Action::ModelSwitch` in the events layer

**Goal:** `/model <id>` typed in the input box and submitted with Enter
produces `Action::ModelSwitch` instead of `Action::Submit`.

**Changes to `src/tui/events.rs`:**

1. Add `ModelSwitch(String)` to the `Action` enum.
2. In `handle_key_event`, on `KeyCode::Enter` when `!is_waiting` and buffer
   non-empty, check for the `/model` prefix **before** the existing submit
   path. See the pseudocode in R3 above.

**Changes to `src/tui/mod.rs` (minimal wiring stub):**

Add an arm to the `match action { ... }` block inside `event_loop`:

```rust
events::Action::ModelSwitch(id) => {
    // Stub: append a placeholder notice. Full impl in Step 4.
    self.chat_history.push(ChatEntry::AssistantDone(vec![
        ChatBlock::Text(format!("\n[/model {id} — switching not yet implemented]\n"))
    ]));
}
```

This keeps the project compiling without the full async provider swap.

**Tests (add to `#[cfg(test)] mod tests` in `src/tui/events.rs`):**

| Test name | Assertion |
|-----------|-----------|
| `slash_model_with_id_returns_model_switch` | buffer = `/model claude-haiku`, Enter → `Action::ModelSwitch("claude-haiku")` |
| `slash_model_no_arg_returns_model_switch_empty` | buffer = `/model`, Enter → `Action::ModelSwitch("")` |
| `slash_model_with_extra_spaces_trims_id` | buffer = `/model   my-id  `, Enter → `Action::ModelSwitch("my-id")` |
| `slash_model_clears_input_buffer` | after dispatch `app.input_buffer.is_empty()` |
| `regular_submit_not_intercepted` | buffer = `hello`, Enter → `Action::Submit("hello")` |
| `slash_model_while_waiting_returns_none` | `is_waiting = true`, buffer = `/model x`, Enter → `Action::None`; buffer unchanged |
| `slash_model_prefix_only_no_trailing_space` | buffer = `/model`, Enter → `Action::ModelSwitch("")` (not Submit) |

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 4 — `handle_model_switch` full implementation

**Goal:** replace the Step 3 stub with the real async method; status bar and
conversation model field update immediately; `RecentModels` persisted.

**Changes to `src/tui/mod.rs`:**

1. Add `use crate::provider::BedrockProvider;` if not already imported (it is
   already used in `main.rs` but not in `tui/mod.rs`; add the import).
2. Replace the stub arm in `event_loop` with:
   ```rust
   events::Action::ModelSwitch(id) => {
       self.handle_model_switch(id).await;
   }
   ```
3. Implement `handle_model_switch` as specified in R4. Critical constraints:
   - Drop the `conv` mutex guard **before** any `.await` to avoid holding the
     lock across an await point.
   - On `Err`, leave `self.provider`, `self.model_name`, and `conv.model` all
     unchanged.
   - `RecentModels` I/O is best-effort: wrap in `if let Ok(...) { ... }`.

**Tests (add to `#[cfg(test)] mod tests` in `src/tui/mod.rs`):**

`BedrockProvider::new` succeeds in construction (no eager credential check)
but calling it in unit tests requires AWS credentials. The safe strategy:

- Test the **empty-id path** directly (no provider construction).
- Test the **error path** by exploiting the fact that `BedrockProvider::new`
  succeeds but the returned provider will fail at call time — construction
  itself is safe.

| Test name | Assertion |
|-----------|-----------|
| `handle_model_switch_empty_shows_current_model` | `app.handle_model_switch("".to_string()).await` → one `ChatEntry::AssistantDone` appended containing current `model_name` string |
| `handle_model_switch_empty_does_not_change_model_name` | `model_name` field unchanged after empty switch |
| `handle_model_switch_empty_does_not_push_to_chat_history_twice` | exactly one entry added to `chat_history` per empty-switch call |
| `model_name_updated_synchronously_before_render` | After `handle_model_switch` returns (not waiting for next event loop tick), `app.model_name` reflects the new value. Verify via the empty-id path that the field is visible immediately. |

Note: do not write a test that constructs a real `BedrockProvider` and expects
the switch to succeed — that requires live AWS credentials. The happy-path
integration check is covered by AC4 in the acceptance criteria (manual/CI
with credentials).

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 5 — `format_model_segment` + status bar polish

**Goal:** long model ids are truncated in the status bar; the new helper is
independently tested.

**Changes to `src/tui/ui.rs`:**

1. Implement `format_model_segment` as specified in R5 (public within the
   crate: `pub(crate)`).
2. In `render_status_bar`, replace the raw `app.model_name` in the format
   string with `format_model_segment(&app.model_name)`.
3. Add a comment to `render_input_line` mentioning the `/model <id>` command
   so it is discoverable in the source (no new widget required).

**Tests (add to `#[cfg(test)] mod tests` in `src/tui/ui.rs`):**

| Test name | Assertion |
|-----------|-----------|
| `format_model_segment_short_name_unchanged` | `"claude-3"` → `"claude-3"` |
| `format_model_segment_exactly_30_chars_unchanged` | 30-char string → unchanged, no `…` |
| `format_model_segment_31_chars_truncated` | 31-char string → `.chars().count() == 30`, ends with `…` |
| `format_model_segment_very_long_name` | 80-char string → ends with `…`, total char count ≤ 30 |
| `format_model_segment_empty_string` | `""` → `""` (no panic) |
| `status_bar_model_name_regression` | headless app has `model_name == "test-model"` (existing behaviour preserved) |

**Compile check:** `cargo test -q 2>&1 | tail -5`

---

### Step 6 — Final wiring verification + integration smoke test

**Goal:** everything compiles clean; all tests pass; `--help` shows `-m`;
headless mode logs the active model.

**Changes to `src/main.rs`:**

1. Add a startup log line in `run_headless` (before session init):
   ```rust
   eprintln!("ap: model: {}", config.provider.model);
   ```
   Place it immediately after the provider construction succeeds so the model
   used is always visible in stderr output.

2. Ensure `run_tui` passes `config.provider.model.clone()` as `model_name` to
   `TuiApp::new` — audit the existing call site; it should already be correct.
   If not, fix it.

**Tests (add to existing `#[cfg(test)] mod tests` in `src/main.rs`):**

| Test name | Assertion |
|-----------|-----------|
| `model_override_flows_into_tui_model_name` | Simulate the override logic: start with `AppConfig::default()`, apply `config.provider.model = "override".to_string()`, assert `config.provider.model == "override"` — confirms the in-`main` pattern is correct |
| `model_flag_overrides_default_model` | `Args::try_parse_from(["ap","--model","claude-3-haiku"])`, apply override to default config, assert model equals `"claude-3-haiku"` and region/backend are still defaults |

**Final clean-build check:**

```
cargo build 2>&1
cargo test 2>&1 | tail -20
cargo clippy -- -D warnings 2>&1 | tail -20
```

All three must produce zero errors and zero warnings.

---

## Acceptance Criteria

- [ ] **AC1** — `cargo build` succeeds with zero errors and zero warnings.
- [ ] **AC2** — `cargo test` passes all tests with zero failures; no test is
  skipped or ignored by this feature's additions.
- [ ] **AC3** — `cargo clippy -- -D warnings` exits 0 (no clippy warnings).
- [ ] **AC4** — `ap --help` output includes `-m, --model <MODEL>`.
- [ ] **AC5** — `ap -m us.anthropic.claude-haiku-4-5 -p "say hi"` logs
  `ap: model: us.anthropic.claude-haiku-4-5` to stderr before the turn runs.
- [ ] **AC6** — `RecentModels::push` is pure: the original `RecentModels`
  value is unchanged and the returned value has the new id at index 0.
- [ ] **AC7** — `RecentModels::push` deduplicates: pushing an id that is
  already present results in exactly one occurrence at index 0.
- [ ] **AC8** — `RecentModels::push` caps at `MAX_RECENT` (10) entries:
  after 11 pushes of distinct ids, `models.len() == 10`.
- [ ] **AC9** — `save_to_then_load_from_roundtrip` test passes: ids survive
  a JSON round-trip in insertion order.
- [ ] **AC10** — Typing `/model claude-haiku` and pressing Enter in the TUI
  produces `Action::ModelSwitch("claude-haiku")`, not `Action::Submit`.
- [ ] **AC11** — Typing `/model` (no argument) and pressing Enter produces
  `Action::ModelSwitch("")`, not `Action::Submit`.
- [ ] **AC12** — `/model` with any content while `is_waiting = true` produces
  `Action::None`; the input buffer is NOT cleared.
- [ ] **AC13** — `handle_model_switch("")` appends exactly one
  `ChatEntry::AssistantDone` to `chat_history` containing the current
  `model_name`, and does not change `model_name`.
- [ ] **AC14** — `format_model_segment` truncates ids longer than 30 chars to
  29 chars + `…` (total char count = 30).
- [ ] **AC15** — `format_model_segment` returns short ids unchanged with no
  `…` appended.
- [ ] **AC16** — `src/lib.rs` contains `pub mod models;` and all tests in
  `src/models.rs` pass.
- [ ] **AC17** — The status bar in `render_status_bar` calls
  `format_model_segment` rather than interpolating `app.model_name` directly.

---

## Implementation Notes for Ralph

**Clippy constraints (enforced at `deny` level):**
- Never use `unwrap()` or `expect()` outside `#[cfg(test)]` blocks.
- Never use `panic!()` outside `#[cfg(test)]` blocks.
- Use `anyhow::Context` (`.context("…")` / `.with_context(|| …)`) on every
  fallible I/O call so errors carry file-path information.

**No new dependencies:** `dirs` (home dir), `serde`, `serde_json`, and
`anyhow` are already in `Cargo.toml`. Do not add `rand`.

**Mutex discipline:** `TuiApp.conv` is `Arc<tokio::sync::Mutex<Conversation>>`.
Always drop the guard before an `.await` point. Pattern:
```rust
let value = {
    let guard = self.conv.lock().await;
    guard.some_field.clone()
};  // guard dropped here — safe to .await after this point
```

**`BedrockProvider` construction is eager but not credential-checking.**
`BedrockProvider::new` loads the AWS SDK config and constructs a `Client`.
It always succeeds regardless of credential availability. Unit tests may call
it freely; it will only fail at the actual `invoke_model_with_response_stream`
call, which never happens in unit tests.

**`Action` enum `PartialEq` derive:** The existing `Action` derives
`PartialEq`. The new `ModelSwitch(String)` variant will be covered
automatically. Ensure `#[derive(Debug, PartialEq)]` remains on the enum.

**Test file placement:** All tests live in inline `#[cfg(test)] mod tests`
blocks inside the same source file as the code under test. Do not create
separate integration test files for this feature.

**Step ordering is strict.** Each step must compile cleanly before the next
begins. Steps 1 and 2 are independent of each other (different files) but
Step 3 depends on Step 1 (`models` module referenced in the stub), and Step 4
depends on Steps 1, 2, and 3.

**`/model` command scope:** The command is only valid in the TUI. Headless
mode (`-p`) does not process slash-commands; it passes the prompt string
directly to `turn()`. The `--model` flag is the headless equivalent.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project
builds clean.
