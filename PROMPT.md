Now I have a thorough understanding of the codebase. Let me write the PROMPT.md.Here is the `PROMPT.md` content written to `~/Projects/ap/ap/PROMPT.md`:

---

## What the prompt covers

**Vision** — Explains the exact UX change: token-by-token rendering in the TUI pane and a dual-mode `Ctrl+C` (cancel-turn vs quit) without losing conversation history.

**Technical requirements (R1–R10)** — Each requirement maps to a concrete Rust type or signature change:
- `R1` — New `turn_streaming()` signature with `CancellationToken` and `mpsc::Sender<TurnEvent>`
- `R2` — `TurnEvent::Cancelled` variant (with all exhaustive-match sites called out)
- `R3–R6` — `cancel_token: Option<CancellationToken>` field in `TuiApp`, new `Action::CancelTurn`, wiring through the event loop
- `R7` — Full `handle_ui_event` arm for `Cancelled` (exact code given)
- `R8` — `tokio-util` dep
- `R9–R10` — Status bar `STREAMING` indicator and help overlay update

**8 ordered steps** — Each step is independently compilable and includes exact file names, the code to write, and any new unit tests to add. The batch `turn()` is never touched, preserving all existing tests.

**14 acceptance criteria** — Every AC maps to a specific `cargo test` name or build command, making it machine-checkable for the Ralph PDD loop.

The prompt ends with the required sentinel:

> Output `LOOP_COMPLETE` when all acceptance criteria are met and the project builds clean.