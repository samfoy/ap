# PROMPT.md — TUI Overhaul: Simple Claude Code / pi Style UI

## Vision

Replace the current busy, broken ratatui TUI with a clean, minimal terminal chat interface modeled after Claude Code and pi. The current UI has layout complexity that doesn't work, and Ctrl+Enter inserts a newline instead of submitting. The new UI should feel like a normal terminal tool: type, press Enter, get a response.

## Requirements

### Layout (simple, top to bottom)
- **Status bar** — single line at top: model name, session name, token count
- **Chat area** — scrollable message history filling remaining height
  - User messages: prefixed with `You: ` 
  - Assistant messages: prefixed with `ap: `, streamed token-by-token
  - Tool calls: shown inline as `[tool: bash]` with output collapsible or just shown inline
- **Input line** — single line at bottom, like a shell prompt

### Input behavior
- **Enter** = submit (no exceptions)
- **Shift+Enter** or typed `\n` escape = literal newline in message if needed
- **Up/Down arrows** = scroll chat history
- **Ctrl+C** = cancel current turn or exit if idle
- No multi-line editor widget (this is what's causing the Ctrl+Enter bug)

### What to remove
- Split panes, side panels, busy borders
- Any ratatui Textarea or multi-line input widget
- Tool call popup/overlay boxes
- Anything that requires layout negotiation

### Reference implementations
- `pi` — single input line, clean scroll, streams response inline
- Claude Code — plain terminal output, input at bottom, no frames

## Technical Plan

1. **Audit `src/tui/`** — identify the current input widget (likely `tui-textarea` or similar) and layout code
2. **Replace input widget** with a simple single-line `String` buffer + raw key event handling
3. **Simplify layout** — `Layout::vertical([Length(1), Fill(1), Length(1)])` for status/chat/input
4. **Chat scroll** — `Vec<ChatLine>` with a scroll offset; Up/Down adjusts offset
5. **Stream rendering** — assistant response appended char-by-char to last chat line as TurnEvents arrive
6. **Remove unused deps** from Cargo.toml if any widget crates become unused

## Acceptance Criteria

- `ap` launches, shows clean single-line input at bottom
- Typing and pressing Enter submits the message
- Response streams inline in the chat area above
- Up/Down scrolls history
- Ctrl+C exits cleanly
- `cargo build` passes with no errors
- `cargo test` passes (204+ tests)

Output LOOP_COMPLETE when all acceptance criteria are met and the project builds clean.
