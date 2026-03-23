Now I have a thorough picture of the existing codebase. Let me write the detailed PROMPT.md:Here is the `PROMPT.md` content written to `~/Projects/ap/ap/PROMPT.md`:

---

The prompt covers:

**Vision** — three-zone layout (status bar + chat area + input bar), no modal modes, no panes, feels like a normal terminal chat tool.

**Guiding Principles** — functional-first, no AppMode, single vertical layout, all existing `handle_ui_event` tests preserved.

**Current State** — exact inventory of the four files being changed vs. the ones left untouched (`theme.rs`, `types.rs`, `turn.rs`, `main.rs`).

**Types and Signatures** — exact Rust signatures for:
- `Action` (6 variants, new `ScrollUp`/`ScrollDown`/`ScrollToBottom`)
- `handle_key_event` with a precise key-binding table
- `status_text(model, turns, last_input_tokens, context_limit) -> String`
- `chat_lines<'a>(history, tool_entries, theme) -> Vec<Line<'a>>`
- `render(frame, app)` with exact `Layout::vertical([Length(1), Min(1), Length(3)])` spec

**4 ordered steps**, each independently compilable:
1. Strip `AppMode`/`selected_tool`/`show_help` from `TuiApp`; rewrite `events.rs` with 11 named tests
2. Rewrite `ui.rs` with `status_text`, `chat_lines`, `render`; 9 named tests
3. Wire scroll actions in `event_loop`; 5 integration tests
4. Final cleanup — dead code, stale imports, `git diff` checks, clippy, grep checks

**15 acceptance criteria** with exact grep commands to verify structural properties, named unit tests to verify behaviour, and `cargo build/test/clippy` gating the whole thing.

**Ends with** the `LOOP_COMPLETE` instruction.