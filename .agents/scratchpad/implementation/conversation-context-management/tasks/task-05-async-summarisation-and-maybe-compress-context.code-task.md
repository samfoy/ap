---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Async Summarisation and maybe_compress_context

## Description
Extend `ap/src/context.rs` with two async functions: `summarise_messages` (streams a summary from the provider) and `maybe_compress_context` (the full compression pipeline). Also add `Conversation::with_messages` builder to `ap/src/types.rs`.

## Background
`summarise_messages` builds a summary prompt from the "to-be-archived" messages, calls `provider.stream_completion`, drains the stream collecting `TextDelta` text, and returns the accumulated string. `maybe_compress_context` orchestrates the full pipeline: estimate tokens → check threshold → find split → summarise → build new messages (summary wrapper + recent tail) → call `conv.with_messages(new_messages)` → return `(Conversation, Some(TurnEvent::ContextSummarized))`.

**Ownership constraint (critical):** `maybe_compress_context` takes owned `Conversation`. The call site MUST clone before calling and keep the clone as a fallback. This function does NOT need to handle the clone — that is the caller's responsibility (Steps 6 and 7).

**MockProvider and ErrorProvider** must be defined in the `#[cfg(test)]` block. Copy the pattern verbatim from `src/turn.rs` tests.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/conversation-context-management/design.md`

**Additional References:**
- `.agents/scratchpad/implementation/conversation-context-management/context.md` (codebase patterns, especially MockProvider in turn.rs tests)
- `.agents/scratchpad/implementation/conversation-context-management/plan.md` (overall strategy)

**Note:** You MUST read the design document before beginning implementation. Read `ap/src/turn.rs`, `ap/src/types.rs`, and `ap/src/provider/mod.rs` in full before implementing.

## Technical Requirements
1. **`ap/src/types.rs`**: Add `pub fn with_messages(mut self, messages: Vec<Message>) -> Self { self.messages = messages; self }` to `impl Conversation`
2. **`ap/src/context.rs`**: Add:
   ```rust
   pub async fn summarise_messages(
       messages: &[Message],
       provider: &dyn Provider,
   ) -> anyhow::Result<String>
   ```
   - Builds a summary prompt from the messages
   - Calls `provider.stream_completion(summary_conv)` and drains the stream
   - Collects all `TurnEvent::TextDelta(text)` into a single `String`
   - Returns `Ok(summary)` or `Err` if the stream produces an error
3. **`ap/src/context.rs`**: Add:
   ```rust
   pub async fn maybe_compress_context(
       conv: Conversation,
       config: &ContextConfig,
       provider: &dyn Provider,
   ) -> anyhow::Result<(Conversation, Option<TurnEvent>)>
   ```
   - If `config.limit.is_none()`, returns `Ok((conv, None))`
   - Estimates tokens; if below `limit * threshold`, returns `Ok((conv, None))`
   - Calls `find_summary_split`; if `None`, returns `Ok((conv, None))`
   - Calls `summarise_messages` for the messages before the split
   - Builds `new_messages`: `[Message::user(summary_text)] + conv.messages[split_idx..]`
   - Calls `conv.with_messages(new_messages)` and returns `Ok((new_conv, Some(TurnEvent::ContextSummarized { .. })))`
4. All 6 new async tests must pass; all existing tests must continue to pass

## Dependencies
- Task 01 (pure functions: `estimate_tokens`, `find_summary_split`)
- Task 04 (`TurnEvent::ContextSummarized` variant must exist)

## Implementation Approach
1. **TDD: Write all 6 failing async tests first** using `#[tokio::test]` in the `#[cfg(test)]` block
2. Define `MockProvider` (returns fixed stream) and `ErrorProvider` (returns error)
3. Add `Conversation::with_messages` to `types.rs`
4. Implement `summarise_messages`
5. Implement `maybe_compress_context`
6. `cargo test context::tests` — all 14 tests (8 + 6) pass
7. `cargo build` — zero warnings

## Acceptance Criteria

1. **summarise_messages_collects_stream**
   - Given `MockProvider` that returns `TextDelta("foo")`, `TextDelta("bar")`, then `TurnEnd`
   - When calling `summarise_messages(&messages, &mock_provider).await`
   - Then the result is `Ok("foobar")`

2. **summarise_messages_provider_error_returns_err**
   - Given `ErrorProvider` that returns an error stream
   - When calling `summarise_messages(&messages, &error_provider).await`
   - Then the result is `Err(...)`

3. **maybe_compress_context_no_op_under_threshold**
   - Given a conversation whose estimated tokens are below `limit * threshold`
   - When calling `maybe_compress_context(conv, &config, &provider).await`
   - Then the result is `Ok((original_conv, None))`

4. **maybe_compress_context_compresses_when_over_threshold**
   - Given a conversation whose estimated tokens are at or above the threshold
   - When calling `maybe_compress_context(conv, &config, &provider).await`
   - Then `result_conv.messages.len() < original_message_count` and `event` is `Some(TurnEvent::ContextSummarized { .. })`

5. **maybe_compress_context_new_messages_start_with_user**
   - Given compression fires (tokens over threshold)
   - When calling `maybe_compress_context(conv, &config, &provider).await`
   - Then `result_conv.messages[0].role == Role::User`

6. **maybe_compress_context_cannot_split_returns_unchanged**
   - Given a conversation with too few messages to satisfy `keep_recent` (find_summary_split returns None)
   - When calling `maybe_compress_context(conv, &config, &provider).await`
   - Then the result is `Ok((original_conv, None))`

7. **All Tests Pass**
   - Given the implementation is complete
   - When running `cargo test context::tests`
   - Then all 14 tests pass and `cargo build` produces zero warnings

## Metadata
- **Complexity**: High
- **Labels**: context-management, async, summarisation, tdd
- **Required Skills**: Rust, async/await, tokio, provider trait
