`PROMPT.md` written to `/Users/sam.painter/Projects/ap/PROMPT.md`.

Here's what it covers and why each choice was made:

---

**Structure overview**

| Section | What it nails down |
|---|---|
| Vision | Why this matters — context window death vs. graceful compression |
| Technical Requirements | Exact Rust signatures for every new public item |
| 7 ordered steps | Each independently compilable; smallest possible diff per step |
| Acceptance Criteria | 12 binary checks + the `LOOP_COMPLETE` termination signal |

---

**Key design decisions captured in the spec**

- **`TurnMiddlewareFn` is sync** → `maybe_compress_context` is a standalone `async fn`, not a middleware closure. Called before `turn()` in both the headless path and the TUI's spawned task.
- **No architectural mutation** — the new `src/context.rs` module is self-contained; all existing types grow minimally (`TurnEvent` gets one new variant, `AppConfig` gets one new field).
- **Token estimation is a pure heuristic** (`chars / 4`) so Step 1 has zero I/O dependencies, making it a safe starting point.
- **Alternating-turn constraint** — `find_summary_split` always advances the split to the first `User` message in the tail; this is tested explicitly (AC 11).
- **TUI status bar** always shows `ctx: XX.Xk`; percentage only when a limit is configured — keeps the bar uncluttered by default.
- **`last_input_tokens`** (not cumulative) tracks *current context size* using the `input_tokens` value the provider already sends via `TurnEvent::Usage`.