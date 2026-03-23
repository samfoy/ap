Now I have a thorough understanding of the codebase. Let me write the PROMPT.md.The `PROMPT.md` has been written to `~/Projects/ap/ap/PROMPT.md` (340 lines, 18 KB). Here's a summary of what it contains:

---

## What's in the PROMPT.md

### Vision
Replace the broken, over-engineered 4-pane modal TUI with a 3-zone chat interface: **status bar → scrollable chat → single-line input**. No modes, no panels, no boxes.

### Technical Requirements
- **Exact Rust types/signatures** for the new `TuiApp` (fields removed: `mode`, `selected_tool`, `show_help`), the new `Action` enum (adds `ScrollUp`, `ScrollDown`, `ScrollToBottom`), and the new pure render helpers (`chat_lines`, `status_text`, `render`).
- **Key binding table** — Enter=submit, Up/Down=scroll, Ctrl+C=quit, Ctrl+L=bottom. No modal switching.
- **Layout spec** — `Layout::vertical([Length(1), Min(1), Length(3)])`. No horizontal splits.
- **Chat rendering rules** — `You: ` prefix in accent bold, `ap: ` prefix in success bold, tool entries as single inline dim annotations (`· tool: bash ✓`).

### 5 Ordered Steps (each independently compilable)
1. **Remove `AppMode`, simplify `TuiApp`** — delete modal fields, rewrite `events.rs` with no-modal bindings + new tests
2. **Rewrite `ui.rs`** — new three-zone layout, `chat_lines()` and `status_text()` as pure testable functions
3. **Wire scroll actions into event_loop** — connect the new `Action` variants, add scroll tests
4. **Integration smoke tests** — `headless_new_ui_state`, `submit_clears_buffer`, `waiting_prevents_submit`
5. **Final cleanup** — dead code, clippy, release build validation

### 11 Acceptance Criteria
Covering: clean build, all tests pass, `AppMode` fully gone, `handle_key_event` behaviour verified, three-zone layout, pure helper functions with tests, fixed-height input bar, no help overlay code, zero new clippy warnings.

### Termination condition
`LOOP_COMPLETE` when all 11 ACs are met and the project builds clean.