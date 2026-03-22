---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Pure Token Estimation and Split Logic

## Description
Create `ap/src/context.rs` with three pure, synchronous functions: `estimate_message_tokens`, `estimate_tokens`, and `find_summary_split`. These are the foundation of the context-management pipeline and have zero I/O dependencies, making them safe to implement and test in isolation.

## Background
The context-management feature needs to detect when a conversation is approaching a token limit and decide where to split the history for summarisation. The token count is a heuristic (`chars / 4`, min 1 per message). The split point must always land on a `User` turn to preserve the alternating-turn requirement of Claude's API.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/conversation-context-management/design.md`

**Additional References:**
- `.agents/scratchpad/implementation/conversation-context-management/context.md` (codebase patterns)
- `.agents/scratchpad/implementation/conversation-context-management/plan.md` (overall strategy)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Create `ap/src/context.rs` as a new file
2. Register `pub mod context;` in `ap/src/lib.rs`
3. Implement `pub fn estimate_message_tokens(msg: &Message) -> u32` — iterate content blocks: `Text { text }` → `text.chars().count() / 4`, `ToolUse { name, input }` → `(name.chars().count() + input.to_string().chars().count()) / 4`, `ToolResult { content, .. }` → `content.chars().count() / 4`; each block contributes at least 1 token; sum all blocks, return max(sum, 1)
4. Implement `pub fn estimate_tokens(messages: &[Message]) -> u32` — sum `estimate_message_tokens` over the slice
5. Implement `pub fn find_summary_split(messages: &[Message], keep_recent: usize) -> Option<usize>` — if `messages.len() <= keep_recent`, return `None`; the candidate index is `messages.len() - keep_recent`; scan forward from candidate until finding a `Message` with `role == Role::User`; if none found, return `None`; otherwise return `Some(idx)`

## Dependencies
- None — this is the first step, all dependencies come from existing `ap` types

## Implementation Approach
1. **TDD: Write all 8 failing tests first** in the `#[cfg(test)]` block at the bottom of `context.rs`
2. Implement `estimate_message_tokens` to pass the first 4 tests
3. Implement `estimate_tokens` (trivial sum)
4. Implement `find_summary_split` to pass the last 4 tests
5. `cargo test context::tests` — all 8 must pass
6. `cargo build` — zero warnings

## Acceptance Criteria

1. **estimate_tokens_empty**
   - Given `&[]` (empty slice)
   - When calling `estimate_tokens(&[])`
   - Then returns `0`

2. **estimate_tokens_text_message**
   - Given a message with a single `Text { text: "hello world" }` content block (11 chars)
   - When calling `estimate_message_tokens(&msg)`
   - Then returns `2` (11/4=2, which is ≥ 1)

3. **estimate_tokens_tool_use**
   - Given a message with `ToolUse { name: "bash", input: json!("ls") }` (4+4=8 chars)
   - When calling `estimate_message_tokens(&msg)`
   - Then returns `2` (8/4=2)

4. **estimate_tokens_tool_result**
   - Given a message with `ToolResult { content: "output\n", .. }` (7 chars)
   - When calling `estimate_message_tokens(&msg)`
   - Then returns `1` (7/4=1, which is ≥ 1)

5. **find_summary_split_too_short**
   - Given 3 messages and `keep_recent=5`
   - When calling `find_summary_split(&msgs, 5)`
   - Then returns `None`

6. **find_summary_split_finds_user**
   - Given 10 messages where `msgs[5]` is a `User` message and `keep_recent=5`
   - When calling `find_summary_split(&msgs, 5)`
   - Then returns `Some(5)`

7. **find_summary_split_skips_to_user**
   - Given 10 messages where `msgs[5]` is `Assistant`, `msgs[6]` is `User`, and `keep_recent=5`
   - When calling `find_summary_split(&msgs, 5)`
   - Then returns `Some(6)`

8. **find_summary_split_no_user_in_tail**
   - Given 10 messages where the tail (indices 5–9) contains only `Assistant` messages and `keep_recent=5`
   - When calling `find_summary_split(&msgs, 5)`
   - Then returns `None`

9. **All Tests Pass**
   - Given the implementation is complete
   - When running `cargo test context::tests`
   - Then all 8 tests pass and `cargo build` produces zero warnings

## Metadata
- **Complexity**: Low
- **Labels**: context-management, pure-functions, tdd
- **Required Skills**: Rust, iterator patterns
