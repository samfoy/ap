---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: turn() Sig Amendment + Decouple TUI from `AgentLoop`

## Description
This task merges two concerns:

**05a — turn() signature amendment:** Change `turn()` to return `Result<(Conversation, Vec<TurnEvent>)>` instead of taking a `tx: &mpsc::Sender<TurnEvent>` parameter. This makes `turn()` a pure function — no side effects, the caller routes events. Update all callers (headless in main.rs, noninteractive tests, turn unit tests, TUI handle_submit).

**06 — TUI decouple:** Rewrite `src/tui/mod.rs` to decouple from `AgentLoop`. The new `TuiApp` holds `Arc<tokio::sync::Mutex<Conversation>>`, `Arc<dyn Provider>`, `Arc<ToolRegistry>`, and `Arc<Middleware>`. On submit it spawns a tokio task calling `turn()` and reads the returned `Vec<TurnEvent>`, sending them to the UI channel. All 7 in-file TUI unit tests updated from `UiEvent::*` to `TurnEvent::*`.

## Background
Currently `turn()` takes `tx: &mpsc::Sender<TurnEvent>` and returns `Result<Conversation>`.
The design amendment makes it pure: return `(Conversation, Vec<TurnEvent>)`, no sender.

Currently `TuiApp` stores `Arc<Mutex<AgentLoop>>` and calls `run_turn()` on submit.
After this task, `TuiApp` is fully independent of `AgentLoop`. `AgentLoop` still exists
(deleted in Task 07) but is no longer used.

## Reference Documentation
**Required:**
- Plan: ap/.agents/scratchpad/implementation/ap-fp-refactor/plan.md

**Additional References:**
- ap/src/turn.rs (current turn() to change)
- ap/src/tui/mod.rs (current TUI to rewrite)
- ap/src/main.rs (headless path to update)
- ap/tests/noninteractive.rs (to update)

## Technical Requirements

### Part A — turn() signature change

1. Change `turn()` signature in `src/turn.rs`:
   ```rust
   // BEFORE:
   pub async fn turn(conv, provider, tools, middleware, tx: &mpsc::Sender<TurnEvent>) -> Result<Conversation>
   
   // AFTER:
   pub async fn turn(conv, provider, tools, middleware) -> Result<(Conversation, Vec<TurnEvent>)>
   ```

2. Change `turn_loop()` and internal helpers to collect `TurnEvent`s into a `Vec<TurnEvent>` and return `(Conversation, Vec<TurnEvent>)`. Remove `mpsc` import from turn.rs.

3. Update `src/main.rs` headless path (`run_headless`):
   - Call `turn(conv.with_user_message(prompt), &provider, &tools, &middleware).await?`
   - Unpack `(new_conv, events)`, iterate events for printing
   - Remove the mpsc channel from the headless path entirely

4. Update `tests/noninteractive.rs`:
   - Remove `mpsc` from the test helper
   - Unpack `(conv, events)` from `turn()` directly
   - All 3 tests still pass

5. Update all turn unit tests in `src/turn.rs` to unpack `(conv, events)` tuple

### Part B — TUI decouple

6. New `TuiApp` struct fields (replace `agent: Arc<Mutex<AgentLoop>>`):
   - `conv: Arc<tokio::sync::Mutex<Conversation>>`
   - `provider: Arc<dyn Provider>`
   - `tools: Arc<ToolRegistry>`
   - `middleware: Arc<Middleware>`
   - Keep all rendering state: `messages`, `tool_events`, `input`, `mode`, `scroll_offset`, `is_waiting`, `ui_rx`, `ui_tx`, `model_name`

7. Update `TuiApp::new(...)` constructor to accept new fields

8. Rewrite `handle_submit` to spawn a tokio task calling `turn()` and sending collected events:
   ```rust
   let conv_clone = Arc::clone(&self.conv);
   let provider_clone = Arc::clone(&self.provider);
   let tools_clone = Arc::clone(&self.tools);
   let middleware_clone = Arc::clone(&self.middleware);
   let tx = self.ui_tx.clone();
   tokio::spawn(async move {
       let c = conv_clone.lock().await.clone().with_user_message(input);
       match turn(c, &*provider_clone, &*tools_clone, &*middleware_clone).await {
           Ok((new_conv, events)) => {
               *conv_clone.lock().await = new_conv;
               for event in events {
                   let _ = tx.send(event).await;
               }
           }
           Err(e) => { let _ = tx.send(TurnEvent::Error(e.to_string())).await; }
       }
   });
   ```

9. Update `handle_ui_event` to match on `TurnEvent` variants (same logic as before for `UiEvent`, just renamed)

10. Update `run_tui()` in `main.rs` to construct `Arc<Middleware>`, `Arc<ToolRegistry>`, `Arc<tokio::sync::Mutex<Conversation>>` and pass to new `TuiApp::new`

11. Update all 7 in-file TUI unit tests: replace `UiEvent::*` with `TurnEvent::*`

12. DO NOT delete `app.rs` or `UiEvent` yet — that is Task 07

## Dependencies
- Task 01: Conversation + TurnEvent in types.rs ✓
- Task 02: turn() in turn.rs ✓
- Task 03: Middleware in middleware.rs ✓
- Task 04: SessionStore::save_conversation ✓
- Task 05: main.rs recipe-style baseline ✓

## Implementation Approach
1. Start with Part A: change turn() sig in turn.rs + fix all turn unit tests
2. Update main.rs headless path to unpack tuple
3. Update noninteractive.rs test helper
4. Confirm `cargo test` still passes with no TUI changes yet
5. Move to Part B: update 7 TUI tests to TurnEvent (RED)
6. Rewrite TuiApp struct + constructor + handle_submit
7. Update run_tui() in main.rs
8. GREEN: `cargo test` all pass
9. `cargo build --release` — zero warnings

## Acceptance Criteria

1. **turn() is a pure function — no sender parameter**
   - Given `src/turn.rs`
   - When examining the `turn()` signature
   - Then it takes `(conv, provider, tools, middleware)` and returns `Result<(Conversation, Vec<TurnEvent>)>`

2. **turn() unit tests unpack tuple**
   - Given the updated turn unit tests
   - When running `cargo test turn`
   - Then all turn tests pass and none reference `mpsc` or `tx`

3. **Headless path works without channel**
   - Given `run_headless` in main.rs
   - When examining the source
   - Then there is no `mpsc::channel` — events come from `turn()`'s return value

4. **TuiApp stores Conversation, not AgentLoop**
   - Given the new TuiApp definition
   - When examining struct fields
   - Then no `agent: Arc<Mutex<AgentLoop>>` field exists

5. **TUI TextChunk event updates conversation display**
   - Given a TuiApp with seeded `ui_rx` containing `TurnEvent::TextChunk("Hello".into())`
   - When `handle_ui_event` processes the event
   - Then `messages` contains "Hello"

6. **TUI ToolStart event updates tool display**
   - Given `TurnEvent::ToolStart { name: "bash".into(), params: json!({}) }`
   - When `handle_ui_event` processes the event
   - Then `tool_events` contains an entry for "bash"

7. **TUI TurnEnd clears waiting state**
   - Given `is_waiting=true` and `TurnEvent::TurnEnd` in the channel
   - When `handle_ui_event` processes the event
   - Then `is_waiting` is false

8. **All 7 TUI tests pass with TurnEvent**
   - Given the updated test module
   - When running `cargo test tui`
   - Then all 7 tests pass using `TurnEvent` variants with zero `UiEvent` references

9. **All tests pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all tests pass

10. **Release build succeeds**
    - Given the implementation is complete
    - When running `cargo build --release`
    - Then zero warnings

## Metadata
- **Complexity**: High
- **Labels**: tui, turn, decoupling, fp-refactor, design-amendment
- **Required Skills**: Rust, async/await, tokio, ratatui
