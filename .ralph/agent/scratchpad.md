# Scratchpad


## Initial Analysis (2026-03-22)

**Codebase location:** `/Users/sam.painter/Projects/ap-worktrees/richer-tui/ap/`

**Current state:**
- `types.rs`: `TurnEvent` has TextChunk, ToolStart, ToolComplete, TurnEnd, Error — NO Usage variant yet
- `turn.rs`: `StreamEvent::TurnEnd` carries input_tokens/output_tokens but discards them
- `tui/mod.rs`: `messages: Vec<String>`, `tool_events: Vec<String>` (flat strings, no structure)
- `tui/events.rs`: Enter submits, no multi-line support
- `tui/ui.rs`: Simple status bar, basic renders

**Plan:** Implement all 5 steps sequentially (each must compile before moving to next):
1. Token Usage - TurnEvent::Usage + status bar
2. Multi-line input - Enter=newline, Ctrl+Enter=submit
3. Structured ToolEntry - replace Vec<String>
4. ChatEntry/ChatBlock + parse_chat_blocks + syntax highlighting
5. scroll_pinned auto-scroll anchor

All work in `ap/src/tui/` and `ap/src/types.rs` and `ap/src/turn.rs`.

**Approach:** Use `design.start` → full workflow since this is a well-defined spec.
The PROMPT.md is extremely detailed - we can go direct to `requirements.complete` 
since it's already a spec, then straight to implementation.

Actually the PROMPT.md is so detailed it's essentially a design doc. I'll emit
`requirements.complete` directly with a reference to PROMPT.md. No need for 
Inquisitor to ask questions.

## Step 3 Complete (2026-03-22)

**Implemented:** Structured ToolEntry replacing Vec<String>

**Changes:**
- `ap/src/types.rs`: Added `is_error: bool` to `TurnEvent::ToolComplete`
- `ap/src/turn.rs`: Pass `is_error: exec_result.is_error` when emitting ToolComplete
- `ap/src/tui/mod.rs`: Added `ToolEntry` struct; replaced `tool_events: Vec<String>` with `tool_entries: Vec<ToolEntry>` + `selected_tool: Option<usize>`; updated `handle_ui_event` and tests
- `ap/src/tui/events.rs`: Added `[`, `]`, `e` key handlers in Normal mode
- `ap/src/tui/ui.rs`: Rewrote `render_tool_panel` with selection highlight, collapsed/expanded rendering; updated help overlay

**Result:** 104 tests pass, 0 warnings, 0 errors.

## Build Unblocked (2026-03-22)

Fixed failing test `tui::ui::tests::code_block_lines_have_dark_bg_style`:
- Test checked `lines[1].style` (line-level style) but header/footer used `Line::from(Span::styled(...))` which only sets span-level style
- Fixed by using `Line::styled(text, yellow)` which sets the line's own `.style` field
- All 122 unit tests + 5 integration tests now pass

All 5 steps confirmed complete:
- Step 1 (Token Usage): TurnEvent::Usage, status bar `Tokens: ↑Xk ↓Yk │ Cost: $N.NNNN`
- Step 2 (Multi-line): Enter=newline, Ctrl+Enter=submit, dynamic height 4-8 rows
- Step 3 (ToolEntry): struct with [ / ] / e navigation
- Step 4 (ChatBlock/ChatEntry + syntax highlighting): parse_chat_blocks, dark bg code blocks
- Step 5 (scroll_pinned): auto-scroll, j/k unpins, G re-pins

Ready for review.

## Step 4 Complete (2026-03-22)

**Implemented:** ChatEntry/ChatBlock + syntax highlighting

**Changes:**
- `ap/src/tui/mod.rs`:
  - Added `ChatBlock { Text(String), Code { lang, content } }` and `ChatEntry { User, AssistantStreaming, AssistantDone }` enums (both derive `Debug, Clone, PartialEq`)
  - Added `pub fn parse_chat_blocks(text: &str) -> Vec<ChatBlock>` with fence-scanning logic
  - Replaced `messages: Vec<String>` with `chat_history: Vec<ChatEntry>` in struct and both constructors
  - Updated `handle_submit` to push `ChatEntry::User(trimmed.clone())`
  - Rewrote `handle_ui_event` for TextChunk/TurnEnd to use `AssistantStreaming` → `AssistantDone` lifecycle
  - Fixed existing tests; added 7 new tests (5 parse_chat_blocks + 2 streaming lifecycle)
- `ap/src/tui/ui.rs`:
  - Rewrote `render_conversation` to render User (with "> " prefix), AssistantStreaming (plain), and AssistantDone (Text=plain, Code=bg(Rgb(30,30,30)))

**Result:** 111 tests pass (was 104), 0 warnings, 0 errors.
