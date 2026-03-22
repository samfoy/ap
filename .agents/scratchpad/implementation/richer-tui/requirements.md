# Requirements — Richer TUI

Source: `/Users/sam.painter/Projects/ap/PROMPT.md`

## Consolidated Requirements

### R1 — Token Usage (Step 1)
- Add `TurnEvent::Usage { input_tokens: u32, output_tokens: u32 }` to `src/types.rs`
- Emit from `turn.rs` on `StreamEvent::TurnEnd` (which already carries token counts, currently discarded)
- `TuiApp` accumulates `total_input_tokens` and `total_output_tokens`
- Status bar shows: `Tokens: ↑Xk ↓Yk │ Cost: $N.NNNN`
- Pricing constants: `COST_PER_M_INPUT = 3.00`, `COST_PER_M_OUTPUT = 15.00`
- Unit test: two Usage events accumulate correctly

### R2 — Multi-line Input (Step 2)
- `Enter` in Insert mode inserts `\n` into `input_buffer`
- `Ctrl+Enter` in Insert mode submits and clears
- Input box height = `clamp(line_count, 2, 6) + 2` (border)
- Cursor positioned at end of last line: `x = chars_after_last_newline`, `y = border + line_index`
- Help overlay updated: `Ctrl+Enter = send`, `Enter = newline`
- Tests: enter adds newline, ctrl+enter submits

### R3 — Structured Tool Entries (Step 3)
- `ToolEntry { name, params: serde_json::Value, result: Option<String>, is_error: bool, expanded: bool }`
- Replace `tool_events: Vec<String>` with `tool_entries: Vec<ToolEntry>` + `selected_tool: Option<usize>`
- `ToolStart` → push entry with `result: None`; `ToolComplete` → find last matching by name with `result.is_none()`, fill result
- `[`/`]` cycle selection, `e` toggles expand
- Collapsed render: `✓/⟳/✗ name`; expanded: `▼ name\n  params: ...\n  result: ...`
- Selected entry highlighted with `bg(Color::DarkGray)`
- Tests: collapsed render, expand toggle, selection wraps

### R4 — Structured Chat + Syntax Highlighting (Step 4)
- `ChatBlock::Text(String)` and `ChatBlock::Code { lang: String, body: String }`
- `ChatEntry::User(String)`, `ChatEntry::AssistantStreaming(String)`, `ChatEntry::AssistantDone(Vec<ChatBlock>)`
- Replace `messages: Vec<String>` with `chat_history: Vec<ChatEntry>`
- Streaming: first TextChunk → push `AssistantStreaming`; subsequent → append; TurnEnd → parse to `AssistantDone`
- `parse_chat_blocks`: splits on ``` fences, unclosed = treat as code, merge adjacent Text
- Rendering: User → `[You]` in Cyan bold + text; AssistantStreaming → plain; AssistantDone Text → plain, Code → `┌─ lang`, body with `bg(Rgb(30,30,30)) fg(White)`, `└────────`
- Tests: ≥4 parser cases + streaming lifecycle

### R5 — Auto-Scroll Anchor (Step 5)
- `scroll_pinned: bool` in `TuiApp`, initialised `true`
- On content events (TextChunk, ToolStart, ToolComplete, TurnEnd): if pinned → `scroll_offset = usize::MAX`
- `j`/`PageDown`: unpin + offset += 3; `k`/`PageUp`: unpin + offset -= 3; `G`: re-pin + offset = usize::MAX

## Constraints
- No new external crates
- Each step must compile+test clean before next
- `clippy::unwrap_used`, `expect_used`, `panic` deny lints stay satisfied outside `#[cfg(test)]`
- `cargo build` zero warnings
