---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: ChatEntry/ChatBlock + Syntax Highlighting

## Description
Introduce `ChatBlock` (Text/Code) and `ChatEntry` (User/AssistantStreaming/AssistantDone) enums. Implement `parse_chat_blocks()` for markdown fence parsing. Replace `messages: Vec<String>` with `chat_history: Vec<ChatEntry>`. Render code blocks with a dark background using ratatui's `bg(Color::Rgb(30, 30, 30))`.

## Background
Currently `TuiApp.messages` is a `Vec<String>` holding flat text. The spec requires structured chat entries with code block syntax highlighting. The streaming lifecycle needs proper `AssistantStreaming` → `AssistantDone` transitions on `TurnEnd`.

`parse_chat_blocks` must be `pub` so it can be unit-tested directly.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/richer-tui/design.md

**Additional References:**
- .agents/scratchpad/implementation/richer-tui/context.md (codebase patterns)
- .agents/scratchpad/implementation/richer-tui/plan.md (overall strategy)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Define in `ap/src/tui/mod.rs`:
   ```rust
   pub enum ChatBlock {
       Text(String),
       Code { lang: String, content: String },
   }
   pub enum ChatEntry {
       User(String),
       AssistantStreaming(String),
       AssistantDone(Vec<ChatBlock>),
   }
   ```
2. Replace `messages: Vec<String>` with `chat_history: Vec<ChatEntry>` in `TuiApp`
3. Update `headless()` constructor to initialise new field
4. Fix all `messages` references in existing tests
5. Implement `pub fn parse_chat_blocks(text: &str) -> Vec<ChatBlock>`:
   - No fence → `[Text(full_text)]`
   - ` ```lang ` opens a code block, ` ``` ` closes it
   - Unclosed fence → treat remaining content as code block
   - Empty input → empty vec
6. Update `handle_ui_event`:
   - `TextChunk`: if last entry is `AssistantStreaming`, append to it; else push new `AssistantStreaming`
   - `TurnEnd`: convert last `AssistantStreaming` to `AssistantDone(parse_chat_blocks(text))`
7. Update `handle_submit` to push `ChatEntry::User(text)`
8. Rewrite `render_conversation` in `ap/src/tui/ui.rs`:
   - `User(text)` → lines prefixed with `"> "`
   - `AssistantStreaming(text)` → plain lines
   - `AssistantDone(blocks)` → Text blocks as plain lines, Code blocks with `bg(Color::Rgb(30, 30, 30))`
9. All code must compile with zero warnings and pass `cargo test`

## Dependencies
- Depends on: task-03 (Step 3 must be complete and compiling)

## Implementation Approach
1. Write failing tests first:
   - `parse_chat_blocks_no_fence`: plain text → `[Text(s)]`
   - `parse_chat_blocks_single_fence`: text + fence → `[Text, Code]`
   - `parse_chat_blocks_with_lang`: ` ```rust ` tag captured in `Code.lang`
   - `parse_chat_blocks_unclosed_fence`: unclosed ``` treated as code block
   - `parse_chat_blocks_empty`: empty string → empty vec
   - `streaming_lifecycle_ends_as_done`: TextChunk × N then TurnEnd → `AssistantDone`
   - `streaming_lifecycle_chunks_appended`: multiple TextChunks land in `AssistantStreaming`
2. Define enums and replace `messages`
3. Implement `parse_chat_blocks`
4. Update `handle_ui_event` and `handle_submit`
5. Rewrite `render_conversation`
6. Run `cargo test` — all tests green

## Acceptance Criteria

1. **parse_chat_blocks: no fence**
   - Given input `"hello world"`
   - When `parse_chat_blocks` is called
   - Then result is `[ChatBlock::Text("hello world")]`

2. **parse_chat_blocks: single fence**
   - Given input `"intro\n\`\`\`\ncode\n\`\`\`\n"`
   - When `parse_chat_blocks` is called
   - Then result is `[Text("intro\n"), Code { lang: "", content: "code\n" }]`

3. **parse_chat_blocks: language tag**
   - Given input with ` ```rust `
   - When `parse_chat_blocks` is called
   - Then the Code block has `lang == "rust"`

4. **parse_chat_blocks: unclosed fence**
   - Given input with an opening ``` but no closing ```
   - When `parse_chat_blocks` is called
   - Then the remaining content is returned as a Code block

5. **parse_chat_blocks: empty input**
   - Given an empty string
   - When `parse_chat_blocks` is called
   - Then the result is an empty vec

6. **Streaming lifecycle**
   - Given multiple `TextChunk` events followed by `TurnEnd`
   - When all events are handled
   - Then `chat_history` last entry is `AssistantDone` with the concatenated text parsed into blocks

7. **Code block styling**
   - Given an `AssistantDone` entry with a `Code` block
   - When `render_conversation` is called
   - Then code lines have `bg(Color::Rgb(30, 30, 30))` style applied

8. **All Tests Pass**
   - Given the complete implementation
   - When running `cargo test` in `ap/`
   - Then all tests pass with zero failures (≥4 `parse_chat_blocks` tests required)

## Metadata
- **Complexity**: Medium
- **Labels**: tui, chat, syntax-highlighting, markdown
- **Required Skills**: Rust, ratatui
