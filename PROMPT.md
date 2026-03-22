The PROMPT.md is written to `/Users/sam.painter/Projects/ap/PROMPT.md`. Here's a summary of what it covers:

---

**Structure:** 5 ordered implementation steps, each independently compilable, covering all 5 backlog items.

**Step 1 — Token Usage:** Adds `TurnEvent::Usage { input_tokens, output_tokens }`, wires it from the existing `StreamEvent::TurnEnd` (which already carries the data but was discarding it), accumulates in `TuiApp`, and renders `Tokens: ↑Xk ↓Yk │ Cost: $N.NNNN` in the status bar using hard-coded Claude 3.5 Sonnet pricing constants.

**Step 2 — Multi-line Input:** Remaps `Enter` → insert `\n`, `Ctrl+Enter` → submit. Makes the input box height dynamic (min 4, max 8 rows) and fixes multi-line cursor positioning.

**Step 3 — Structured Tool Entries:** Replaces `Vec<String>` tool events with a `ToolEntry { name, params, result, is_error, expanded }` struct. Adds `[`/`]` for selection and `e` to toggle expansion, with collapsed/expanded rendering.

**Step 4 — Syntax-Highlighted Conversation:** Introduces `ChatBlock` (Text/Code) and `ChatEntry` (User/AssistantStreaming/AssistantDone) enums. Specifies `parse_chat_blocks()` rules for markdown fences, and maps code blocks to `bg(Color::Rgb(30,30,30))` styled ratatui `Line`s.

**Step 5 — Auto-Scroll Anchor:** Adds `scroll_pinned: bool`, auto-scrolling to `usize::MAX` on new content, unpinned by `j`/`k`, re-pinned by `G`.

**13 acceptance criteria** with specific, testable conditions for the loop's `LOOP_COMPLETE` signal.