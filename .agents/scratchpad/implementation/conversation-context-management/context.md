# Context — conversation-context-management Explorer

## Summary

The codebase is clean, functional-first Rust with clear patterns. Every integration point
for the new feature has a clear existing analogue.

---

## Integration Points

### 1. `src/context.rs` (new module)

**Dependencies needed:**
- `crate::provider::{Message, MessageContent, Role, Provider, StreamEvent, ProviderError}`
- `crate::types::{Conversation, TurnEvent}`
- `futures::StreamExt`
- `anyhow::{anyhow, Result}`

**Summary prompt builds a single `Message::user(prompt_text)`.** Call provider with:
```rust
provider.stream_completion(&[summary_message], &[], None)
```

**`with_messages` builder** — add to `Conversation` in `types.rs`:
```rust
pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
    self.messages = messages;
    self
}
```

### 2. `src/config.rs` — ContextConfig

Add `ContextConfig` struct with `#[serde(default)]`. Add `context: ContextConfig` field
to `AppConfig` (with `#[serde(default)]` — so it serializes to session files and
deserializes missing keys as defaults). Add `[context]` overlay block to `overlay_from_table`.

**Fields:**
```rust
pub struct ContextConfig {
    pub limit: Option<u32>,          // default None
    pub keep_recent_messages: usize, // default 20
    pub summarize_threshold: f64,    // default 0.8
}
```

**TOML overlay:**
- `limit`: `toml::Value::Integer(v)` → `u32::try_from(*v).ok()`
- `keep_recent_messages`: same as `skills.max_injected` pattern
- `summarize_threshold`: `toml::Value::Float(v)` → `*v`

### 3. `src/main.rs` — CLI arg and headless wiring

**Args struct** — add:
```rust
#[arg(long = "context-limit")]
context_limit: Option<u32>,
```

**`run_headless`** — receives `context_limit` (override config). Must clone `conv_with_msg`
before calling `maybe_compress_context`. Pass `config.context` (with limit overridden).

**`route_headless_events`** — add match arm for `TurnEvent::ContextSummarized` to
print a log to stderr. Without it Rust will error on non-exhaustive match.

### 4. `src/tui/mod.rs`

**New fields on TuiApp:**
```rust
pub last_input_tokens: u32,      // replaces (not accumulates)
pub context_limit: Option<u32>,  // from config
```

**`TuiApp::new`** — design says to add `context_limit: Option<u32>` as new parameter.
The one call site in `run_tui` must be updated. (The `headless()` tests are NOT affected
because they call `headless()` not `new()`.)

**IMPORTANT:** `TuiApp::new` signature change affects `run_tui` in `main.rs` only.
The 23 `headless()` calls in tests are unaffected.

**`handle_ui_event`:**
- `Usage` → add `self.last_input_tokens = input_tokens` (keep existing accumulation too)
- New arm for `ContextSummarized` → push notice + set `self.last_input_tokens = tokens_after`

**`handle_submit` spawned task** — after context compression:
- Clone `conv_with_msg` before passing to `maybe_compress_context`
- On `Some(evt)` → `tx.send(evt).await`

### 5. `src/tui/ui.rs` — render_status_bar

Append to status bar text:
```rust
let ctx_k = app.last_input_tokens as f64 / 1_000.0;
let ctx_str = match app.context_limit {
    None => format!("ctx: {:.1}k", ctx_k),
    Some(lim) => {
        let lim_k = lim as f64 / 1_000.0;
        let pct = (app.last_input_tokens as f64 / lim as f64) * 100.0;
        format!("ctx: {:.1}k/{:.0}k ({:.0}%)", ctx_k, lim_k, pct)
    }
};
```

### 6. `src/lib.rs`

Add `pub mod context;`

---

## Key Constraints Discovered

### Constraint 1: TurnEvent non-exhaustive match
`route_headless_events` has a `match event { ... }` over all `TurnEvent` variants.
After adding `TurnEvent::ContextSummarized`, the Builder MUST add a match arm here.
If not, `cargo build` will fail with non-exhaustive patterns (project uses `#[deny(...)]`).

### Constraint 2: TuiApp::new signature
The current `TuiApp::new` takes 5 args. Adding `context_limit` makes it 6.
Only one call site: `run_tui` in `main.rs`. This is safe to update.

### Constraint 3: serde backward compat for AppConfig.context
`Conversation` is persisted to session files (via `SessionStore::save_conversation`).
Adding `context: ContextConfig` with `#[serde(default)]` means old session files without
`"context"` key will deserialize correctly with all-default values. ✓

### Constraint 4: Alternating turns in summarise_messages output
`summarise_messages` returns a `Conversation` where `new_messages[0]` is a `User` message
(the summary wrapped in `Message::user(...)`). The Builder must verify this satisfies
Bedrock's alternating turn requirement.

### Constraint 5: find_summary_split uses keep_recent from ContextConfig
Default `keep_recent_messages = 20`. The split scans from `len - keep_recent` forward to
find the first `User` message. If `len <= keep_recent`, returns `None` (no compression needed).

### Constraint 6: ContextConfig not Serialize/Deserialize if stored in Conversation
Wait — `AppConfig` IS `Serialize/Deserialize` and is stored in `Conversation`. If we add
`context: ContextConfig` with `#[serde(default)]`, it WILL be serialized to session files.
This is intentional and fine for `limit`, `keep_recent_messages`, `summarize_threshold`.

---

## Mock Strategy for context.rs Tests

Copy `MockProvider` and `ErrorProvider` from `turn.rs` into `#[cfg(test)]` in `context.rs`.

For `summarise_messages_collects_stream`: configure `MockProvider` to emit:
```
TextDelta("summary text"),
TurnEnd { stop_reason: "end_turn", input_tokens: 5, output_tokens: 10 }
```

For `maybe_compress_context_compresses_when_over_threshold`: set up a `Conversation` with
many messages whose total chars/4 exceeds `limit * threshold`. Configure `MockProvider`
to return a summary. Verify returned conversation has fewer messages and first message is User.

---

## Test Call Site Count — Confirmed

- `TuiApp::headless()` in `ap/src/tui/mod.rs`: **23 call sites**
- `TuiApp::headless()` in `ap/src/tui/ui.rs`: **1 call site** (in `make_app()` helper)
- Total: **24 call sites** — all safe, `headless()` unchanged

---

## Implementation Order Verification

Step 1 → `context.rs` pure fns + 8 tests → zero deps on other changes ✓
Step 2 → `ContextConfig` + `AppConfig.context` + 5 tests → needs Step 1 module to exist ✓
Step 3 → `--context-limit` CLI flag → needs Step 2 for config field ✓
Step 4 → `TurnEvent::ContextSummarized` + TUI fields + 6 tests → needs Step 2-3 ✓
Step 5 → `summarise_messages` + `maybe_compress_context` + 6 tests → needs Step 4 ✓
Step 6 → Wire headless path → needs Step 5 ✓
Step 7 → Wire TUI path + 1 test → needs Step 6 ✓
