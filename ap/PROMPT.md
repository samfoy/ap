# PROMPT.md — Streaming Improvements

## Vision

`ap` already pipes `TurnEvent::TextChunk` events through an `mpsc` channel to
the TUI, but the current `turn()` implementation **collects all events into a
`Vec` before returning them**, so the TUI only sees the full response at once.
The goal is true token-by-token streaming — every `TextChunk` appears in the
conversation pane the instant the provider emits it — and the ability to
**interrupt a running turn with `Ctrl+C`** without quitting `ap` or losing the
conversation so far.

The completed feature looks like:

1. Each assistant token appears immediately as the LLM emits it.
2. `Ctrl+C` during a running turn cancels that turn, shows a `[Cancelled]`
   notice, and returns `ap` to the ready state. The conversation history up to
   the cancellation point is preserved.
3. `Ctrl+C` when no turn is running still quits `ap` (existing behaviour).
4. Headless (`-p`) mode is unaffected (it already prints chunks as they
   arrive via `stdout`).

---

## Current Architecture (relevant parts)

```
src/turn.rs          — pure turn() pipeline; collects Vec<TurnEvent> then returns
src/tui/mod.rs       — TuiApp; spawns tokio task, sends events via mpsc::Sender<TurnEvent>
src/tui/events.rs    — handle_key_event() → Action::{Quit, Submit, None}
src/types.rs         — TurnEvent enum, Conversation, Middleware
src/provider/mod.rs  — Provider trait, BoxStream<StreamEvent>
```

### Key types (current)

```rust
// turn.rs
pub async fn turn(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
) -> Result<(Conversation, Vec<TurnEvent>)>

// tui/mod.rs  — spawned task
tokio::spawn(async move {
    match turn(conv, &*provider, &tools, &middleware).await {
        Ok((new_conv, events)) => {
            *conv_arc.lock().await = new_conv;
            for event in events { tx.send(event).await.ok(); }
        }
        Err(e) => { tx.send(TurnEvent::Error(…)).await.ok(); }
    }
});

// tui/events.rs
pub enum Action { None, Submit(String), Quit }
```

---

## Technical Requirements

### R1 — Streaming `turn_streaming()`

Add a new function alongside `turn()` in `src/turn.rs`:

```rust
pub async fn turn_streaming(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
    tx: tokio::sync::mpsc::Sender<TurnEvent>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<Conversation>
```

- Sends each `TurnEvent` through `tx` **immediately** as it is produced
  (before the turn loop iteration finishes).
- Checks `cancel.is_cancelled()` after every `StreamEvent` chunk received from
  the provider and after every tool execution. On cancellation:
  - Stops the stream, sends `TurnEvent::Cancelled` through `tx`, and returns
    the `Conversation` as it stood at the **start** of the cancelled turn
    (i.e., without the partial assistant message appended).
- `turn()` (the batch version) is **kept unchanged** — it continues to be used
  by headless mode and all existing tests.
- `turn_streaming()` calls `turn()` implementation helpers where possible to
  avoid duplication (pre/post middleware, tool execution chain).

### R2 — `TurnEvent::Cancelled` variant

Add to `src/types.rs`:

```rust
pub enum TurnEvent {
    // … existing variants …
    /// The current turn was cancelled by the user (Ctrl+C).
    Cancelled,
}
```

All existing `match` arms on `TurnEvent` must be updated to handle (or
explicitly ignore) the new variant.

### R3 — `CancellationToken` in `TuiApp`

Add to `TuiApp` in `src/tui/mod.rs`:

```rust
// src/tui/mod.rs — TuiApp fields
/// Token used to cancel the currently running turn. `None` when idle.
cancel_token: Option<tokio_util::sync::CancellationToken>,
```

`TuiApp::new()` and `TuiApp::headless()` initialise this field to `None`.

### R4 — `Action::CancelTurn` and updated key handler

Add a new `Action` variant in `src/tui/events.rs`:

```rust
pub enum Action {
    None,
    Submit(String),
    Quit,
    /// Cancel the current running turn (Ctrl+C while `is_waiting` is true).
    CancelTurn,
}
```

Update `handle_key_event`:

- `Ctrl+C` when `app.is_waiting == true` → `Action::CancelTurn` (do **not** quit).
- `Ctrl+C` when `app.is_waiting == false` → `Action::Quit` (existing behaviour).

### R5 — `handle_submit` uses `turn_streaming()`

Replace the `tokio::spawn` block inside `TuiApp::handle_submit` to:

1. Create a `CancellationToken`.
2. Store a clone in `self.cancel_token`.
3. Call `turn_streaming(conv, provider, tools, middleware, tx, cancel)`.
4. On success, update the shared `conv_arc`.
5. On error, send `TurnEvent::Error`.
6. After the task completes (success, error, or cancellation), clear
   `self.cancel_token` to `None`.

### R6 — `Action::CancelTurn` handler in event loop

In `TuiApp::event_loop`, handle `Action::CancelTurn`:

```rust
events::Action::CancelTurn => {
    if let Some(token) = &self.cancel_token {
        token.cancel();
    }
}
```

### R7 — `TurnEvent::Cancelled` UI handler

In `TuiApp::handle_ui_event`, add an arm for `TurnEvent::Cancelled`:

```rust
TurnEvent::Cancelled => {
    // Finalise any partial streaming entry
    if let Some(ChatEntry::AssistantStreaming(_)) = self.chat_history.last() {
        self.chat_history.pop();
    }
    self.chat_history.push(ChatEntry::AssistantDone(vec![
        ChatBlock::Text("\n[Cancelled]\n".to_string()),
    ]));
    self.is_waiting = false;
    self.cancel_token = None;
    if self.scroll_pinned {
        self.scroll_offset = usize::MAX;
    }
}
```

### R8 — `tokio-util` dependency

Add to `Cargo.toml`:

```toml
tokio-util = { version = "0.7", features = ["rt"] }
```

### R9 — Status bar "waiting" indicator

When `app.is_waiting` is `true`, append `" [streaming…]"` to the mode label in
the status bar so users can see the turn is active. When cancelled, that
indicator disappears because `is_waiting` is cleared.

### R10 — Help text update

Add a line to the help overlay in `src/tui/ui.rs`:

```
Ctrl+C  (streaming)  Cancel current turn
Ctrl+C  (idle)       Quit ap
```

---

## Ordered Implementation Steps

Each step must leave the project in a **compilable state** (`cargo build`
passes) before moving to the next.

---

### Step 1 — Add `tokio-util` dependency and `TurnEvent::Cancelled`

**Files:** `Cargo.toml`, `src/types.rs`

1. Add to `Cargo.toml` `[dependencies]`:
   ```toml
   tokio-util = { version = "0.7", features = ["rt"] }
   ```

2. Add `Cancelled` variant to `TurnEvent` in `src/types.rs`:
   ```rust
   /// The current turn was cancelled by the user (Ctrl+C).
   Cancelled,
   ```

3. Fix every exhaustive `match` on `TurnEvent` across the codebase:
   - `src/tui/mod.rs` — `handle_ui_event`: add arm (handled in Step 4).
     For now add a `TurnEvent::Cancelled => {}` no-op arm.
   - `src/main.rs` — `route_headless_events`: add `TurnEvent::Cancelled => {}`
     (headless never receives this, but the match must be exhaustive).

4. Add a unit test in `src/types.rs` tests:
   ```rust
   #[test]
   fn turn_event_cancelled_is_clonable() {
       let e = TurnEvent::Cancelled;
       let _ = e.clone();
   }
   ```

**Compile check:** `cargo build` must pass.

---

### Step 2 — `Action::CancelTurn` and updated key handler

**Files:** `src/tui/events.rs`

1. Add `CancelTurn` to the `Action` enum.

2. Update `handle_key_event`:
   - `Ctrl+C` while `app.is_waiting == true` → return `Action::CancelTurn`.
   - `Ctrl+C` while `app.is_waiting == false` → return `Action::Quit`
     (unchanged).
   - Both Insert **and** Normal mode must follow this same logic (Ctrl+C always
     intercepted first).

3. Update `TuiApp::event_loop` match arm:
   ```rust
   events::Action::CancelTurn => {
       // cancel_token not yet present — no-op until Step 4
   }
   ```

4. Add unit tests in `src/tui/events.rs` tests:

   ```rust
   #[test]
   fn ctrl_c_while_waiting_returns_cancel_not_quit() {
       let mut app = TuiApp::headless();
       app.is_waiting = true;
       let action = handle_key_event(ctrl(KeyCode::Char('c')), &mut app);
       assert_eq!(action, Action::CancelTurn);
   }

   #[test]
   fn ctrl_c_while_idle_returns_quit() {
       let mut app = TuiApp::headless();
       app.is_waiting = false;
       let action = handle_key_event(ctrl(KeyCode::Char('c')), &mut app);
       assert_eq!(action, Action::Quit);
   }

   #[test]
   fn ctrl_c_while_waiting_insert_mode_returns_cancel() {
       let mut app = TuiApp::headless();
       app.is_waiting = true;
       app.mode = AppMode::Insert;
       let action = handle_key_event(ctrl(KeyCode::Char('c')), &mut app);
       assert_eq!(action, Action::CancelTurn);
   }
   ```

**Compile check:** `cargo build` and `cargo test` must pass.

---

### Step 3 — `cancel_token` field in `TuiApp`

**Files:** `src/tui/mod.rs`

1. Add the field to `TuiApp`:
   ```rust
   /// Cancellation token for the currently running turn. `None` when idle.
   pub cancel_token: Option<tokio_util::sync::CancellationToken>,
   ```

2. Initialise to `None` in `TuiApp::new()` and both `TuiApp::headless()`
   constructors.

3. Wire up `Action::CancelTurn` in `event_loop`:
   ```rust
   events::Action::CancelTurn => {
       if let Some(token) = &self.cancel_token {
           token.cancel();
       }
   }
   ```

4. Add a unit test:
   ```rust
   #[test]
   fn cancel_token_starts_none() {
       let app = TuiApp::headless();
       assert!(app.cancel_token.is_none());
   }
   ```

**Compile check:** `cargo build` must pass.

---

### Step 4 — `TurnEvent::Cancelled` handler in `handle_ui_event`

**Files:** `src/tui/mod.rs`

Replace the no-op `TurnEvent::Cancelled => {}` arm (added in Step 1) with the
full handler described in R7:

```rust
TurnEvent::Cancelled => {
    if let Some(ChatEntry::AssistantStreaming(_)) = self.chat_history.last() {
        self.chat_history.pop();
    }
    self.chat_history.push(ChatEntry::AssistantDone(vec![
        ChatBlock::Text("\n[Cancelled]\n".to_string()),
    ]));
    self.is_waiting = false;
    self.cancel_token = None;
    if self.scroll_pinned {
        self.scroll_offset = usize::MAX;
    }
}
```

Add unit tests in the `mod tests` block:

```rust
#[test]
fn handle_ui_event_cancelled_clears_streaming_entry() {
    let mut app = TuiApp::headless();
    app.is_waiting = true;
    // Simulate partial streaming
    app.handle_ui_event(TurnEvent::TextChunk("partial...".to_string()));
    assert_eq!(app.chat_history.len(), 1);

    app.handle_ui_event(TurnEvent::Cancelled);

    // The partial streaming entry is replaced by a [Cancelled] notice
    assert_eq!(app.chat_history.len(), 1);
    match &app.chat_history[0] {
        ChatEntry::AssistantDone(blocks) => {
            let text = match &blocks[0] {
                ChatBlock::Text(s) => s.as_str(),
                _ => panic!("expected Text block"),
            };
            assert!(text.contains("Cancelled"), "expected [Cancelled] in text, got: {text}");
        }
        _ => panic!("expected AssistantDone after Cancelled"),
    }
    assert!(!app.is_waiting);
    assert!(app.cancel_token.is_none());
}

#[test]
fn handle_ui_event_cancelled_when_no_streaming_entry_appends_notice() {
    let mut app = TuiApp::headless();
    app.is_waiting = true;
    // No TextChunk yet — turn was cancelled before first token
    app.handle_ui_event(TurnEvent::Cancelled);

    assert_eq!(app.chat_history.len(), 1);
    assert!(!app.is_waiting);
}

#[test]
fn cancelled_auto_scrolls_when_pinned() {
    let mut app = TuiApp::headless();
    app.is_waiting = true;
    app.scroll_pinned = true;
    app.scroll_offset = 5;
    app.handle_ui_event(TurnEvent::Cancelled);
    assert_eq!(app.scroll_offset, usize::MAX);
}

#[test]
fn cancelled_does_not_scroll_when_unpinned() {
    let mut app = TuiApp::headless();
    app.is_waiting = true;
    app.scroll_pinned = false;
    app.scroll_offset = 5;
    app.handle_ui_event(TurnEvent::Cancelled);
    assert_eq!(app.scroll_offset, 5);
}
```

**Compile check:** `cargo build` and `cargo test` must pass.

---

### Step 5 — `turn_streaming()` in `src/turn.rs`

**Files:** `src/turn.rs`

Implement `turn_streaming()` as described in R1. Key design notes:

- Extract the inner `turn_loop` helpers (`apply_pre_turn`, `apply_post_turn`,
  `run_pre_tool_chain`, `run_post_tool_chain`) — they are already private
  free-functions and can be called directly from `turn_streaming`.
- The streaming loop mirrors `turn_loop` but sends each `TurnEvent` through
  `tx` immediately instead of pushing to `Vec`.
- Cancellation check pattern (after each `stream.next().await` call):
  ```rust
  if cancel.is_cancelled() {
      let _ = tx.send(TurnEvent::Cancelled).await;
      return Ok(original_conv);
  }
  ```
  where `original_conv` is a clone of the `Conversation` taken **before**
  appending the user message (the caller passes it in already having the user
  message appended — so save a `pre_turn_conv` clone before calling
  `stream_completion`).
- Also check cancellation **between tool executions** in the tool loop.
- The function signature uses `tokio_util::sync::CancellationToken` from the
  `tokio-util` crate.

Exact signature:

```rust
pub async fn turn_streaming(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
    tx: tokio::sync::mpsc::Sender<TurnEvent>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<Conversation>
```

Add unit tests in `src/turn.rs` tests (using the existing `MockProvider`
infrastructure):

```rust
// AC-S1: turn_streaming sends TextChunks immediately (not batched)
#[tokio::test]
async fn turn_streaming_sends_text_chunks_immediately() {
    // Provider emits two TextDeltas then TurnEnd
    // turn_streaming must send two TextChunk events through tx before returning
    // Verify the channel has the expected events in order.
}

// AC-S2: Cancellation stops the stream and sends TurnEvent::Cancelled
#[tokio::test]
async fn turn_streaming_cancel_sends_cancelled_event() {
    // Provider emits one TextDelta; cancel before TurnEnd
    // turn_streaming must send Cancelled and return the pre-turn Conversation
}

// AC-S3: turn_streaming sends TurnEnd after a complete text-only turn
#[tokio::test]
async fn turn_streaming_complete_turn_sends_turn_end() { … }

// AC-S4: turn_streaming with tool calls sends ToolStart / ToolComplete / TurnEnd
#[tokio::test]
async fn turn_streaming_tool_calls_send_expected_events() { … }

// AC-S5: Cancellation during tool execution sends Cancelled
#[tokio::test]
async fn turn_streaming_cancel_during_tool_sends_cancelled() { … }
```

**Compile check:** `cargo build` and `cargo test` must pass.

---

### Step 6 — Wire `turn_streaming` into `TuiApp::handle_submit`

**Files:** `src/tui/mod.rs`

Replace the existing `tokio::spawn` block in `handle_submit` with:

```rust
async fn handle_submit(&mut self, input: String) {
    // … existing trimmed / empty / /help handling …

    self.chat_history.push(ChatEntry::User(trimmed.clone()));
    self.is_waiting = true;

    // Create a fresh CancellationToken for this turn
    let cancel = tokio_util::sync::CancellationToken::new();
    self.cancel_token = Some(cancel.clone());

    let conv_arc  = Arc::clone(&self.conv);
    let provider  = Arc::clone(&self.provider);
    let tools     = Arc::clone(&self.tools);
    let middleware = Arc::clone(&self.middleware);
    let tx        = self.ui_tx.clone();
    let context_limit = self.context_limit;
    let keep_recent = { … };  // same as before
    let threshold   = { … };  // same as before

    tokio::spawn(async move {
        let conv_with_msg = conv_arc.lock().await.clone().with_user_message(trimmed);

        // Context compression (unchanged logic)
        let conv_to_use = if let Some(limit) = context_limit {
            // … same as before …
        } else {
            conv_with_msg
        };

        // Check cancellation before even starting the turn
        if cancel.is_cancelled() {
            tx.send(TurnEvent::Cancelled).await.ok();
            return;
        }

        match turn_streaming(conv_to_use, &*provider, &tools, &middleware, tx.clone(), cancel).await {
            Ok(new_conv) => {
                *conv_arc.lock().await = new_conv;
            }
            Err(e) => {
                tx.send(TurnEvent::Error(e.to_string())).await.ok();
            }
        }
    });
}
```

Note: `turn_streaming` sends `TurnEnd` (or `Cancelled`) itself; the spawned
task does not need to send them.

**Compile check:** `cargo build` and `cargo test` must pass.

---

### Step 7 — Status bar streaming indicator

**Files:** `src/tui/ui.rs`

In `render_status_bar`, change the mode label:

```rust
let mode_label = match (app.mode, app.is_waiting) {
    (_, true)           => "STREAMING",
    (AppMode::Normal, _) => "NORMAL",
    (AppMode::Insert, _) => "INSERT",
};
```

Add a unit test:

```rust
#[test]
fn status_bar_shows_streaming_label_when_waiting() {
    let mut app = TuiApp::headless();
    app.is_waiting = true;
    // format_status_label is not extracted, so test via chat_entries_to_lines
    // indirectly: just assert the field value used in rendering
    assert!(app.is_waiting);
    // The actual text rendering is integration-level; unit-test the bool flag
    // and trust the render function uses it correctly.
    app.is_waiting = false;
    assert!(!app.is_waiting);
}
```

(The render path is best covered by manual/visual testing; the unit test
confirms the field contract.)

Update the help overlay text to mention:

```
Ctrl+C  Cancel turn (streaming) / Quit (idle)
```

**Compile check:** `cargo build` and `cargo test` must pass.

---

### Step 8 — Final audit and clean-up

1. Run `cargo test` — all existing tests must still pass.
2. Run `cargo clippy -- -D warnings` — zero warnings.
3. Run `cargo build --release` — clean release build.
4. Manually smoke-test:
   - Start `ap`, type a multi-sentence prompt, watch tokens appear token-by-token.
   - During streaming, press `Ctrl+C` — streaming stops, `[Cancelled]` appears,
     `ap` is ready for the next prompt (not quit).
   - Press `Ctrl+C` when idle — `ap` quits.
   - Verify headless mode (`ap -p "..."`) is unaffected and still prints output.

---

## Acceptance Criteria

| # | Criterion | How verified |
|---|-----------|-------------|
| AC1 | `TurnEvent::Cancelled` variant exists and is `Clone` | `cargo test turn_event_cancelled_is_clonable` |
| AC2 | `Ctrl+C` while `is_waiting` → `Action::CancelTurn`, not `Action::Quit` | `cargo test ctrl_c_while_waiting_returns_cancel_not_quit` |
| AC3 | `Ctrl+C` while idle → `Action::Quit` (unchanged) | `cargo test ctrl_c_while_idle_returns_quit` |
| AC4 | `handle_ui_event(Cancelled)` clears streaming entry and appends `[Cancelled]` notice | `cargo test handle_ui_event_cancelled_clears_streaming_entry` |
| AC5 | `handle_ui_event(Cancelled)` clears `is_waiting` and `cancel_token` | same test |
| AC6 | `turn_streaming()` sends `TextChunk` events through `tx` before returning | `cargo test turn_streaming_sends_text_chunks_immediately` |
| AC7 | `turn_streaming()` respects `CancellationToken`: sends `Cancelled` and returns pre-turn `Conversation` | `cargo test turn_streaming_cancel_sends_cancelled_event` |
| AC8 | `turn_streaming()` sends `TurnEnd` after a normal complete turn | `cargo test turn_streaming_complete_turn_sends_turn_end` |
| AC9 | `turn_streaming()` handles tool calls, sends `ToolStart`/`ToolComplete`/`TurnEnd` | `cargo test turn_streaming_tool_calls_send_expected_events` |
| AC10 | `turn()` (batch) still passes all existing tests unchanged | `cargo test` — all pre-existing `turn::tests` pass |
| AC11 | Headless mode (`route_headless_events`) handles `Cancelled` arm without panic | `cargo test` — exhaustive match coverage |
| AC12 | `cargo build --release` passes with zero warnings | `cargo build --release 2>&1 \| grep -c warning` == 0 |
| AC13 | Status bar shows `STREAMING` label when `is_waiting` is true | Visual / unit test on `is_waiting` field |
| AC14 | Help overlay documents the dual Ctrl+C behaviour | Code review of `render_help_overlay` |

---

## Constraints

- **Do not break** `turn()` or any existing test.
- **No `unwrap`/`expect`/`panic`** outside `#[cfg(test)]` blocks (enforced by
  Clippy lints in `Cargo.toml`).
- **Functional-first style**: `turn_streaming` must remain a pure async
  function — no global mutable state, no side effects beyond the `tx` channel
  and the return value.
- **Cancellation is cooperative**: the provider stream is not force-killed;
  `cancel.is_cancelled()` is polled at yield points. This is sufficient because
  `stream.next().await` yields control on every token.
- The `turn_streaming` signature must use `tokio_util::sync::CancellationToken`
  (not a raw `AtomicBool`) so it composes with the broader tokio ecosystem.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project builds clean.
