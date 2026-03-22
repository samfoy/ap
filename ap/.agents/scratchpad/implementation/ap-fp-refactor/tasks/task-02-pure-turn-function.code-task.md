---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Pure `turn()` Function in `src/turn.rs`

## Description
Implement the pure async `turn()` function in a new `src/turn.rs` module. This is the heart of the FP refactor — a stateless pipeline that takes an immutable `Conversation` and returns a new `Conversation` with the turn appended. `AgentLoop` continues to exist; this step is additive.

## Background
The `turn()` function replaces `AgentLoop::run_turn()`. It streams LLM completions, collects tool calls, executes tools through the middleware chain, and returns the updated conversation. The caller is responsible for appending the user message before calling `turn()`.

## Reference Documentation
**Required:**
- Design/Plan: ap/.agents/scratchpad/implementation/ap-fp-refactor/plan.md

**Additional References:**
- ap/.agents/scratchpad/implementation/ap-fp-refactor/context.md (codebase patterns)

**Note:** You MUST read the plan document before beginning implementation. Pay particular attention to the Step 2 section describing the internal pipeline and the note about caller responsibility for appending the user message.

## Technical Requirements
1. Create `src/turn.rs` with:
   ```rust
   pub async fn turn(
       conv: Conversation,
       provider: &dyn Provider,
       tools: &ToolRegistry,
       middleware: &Middleware,
       tx: &tokio::sync::mpsc::Sender<TurnEvent>,
   ) -> anyhow::Result<Conversation>
   ```
2. Internal pipeline (in order):
   - Apply `middleware.pre_turn` chain → possibly modified Conversation
   - `stream_completion` loop: stream provider events → emit `TextChunk` via tx, collect tool use blocks
   - Apply `middleware.post_turn` chain
   - Collect tool calls from stream
   - For each ToolCall: run `middleware.pre_tool` chain → execute tool or skip → run `middleware.post_tool` chain
   - Emit `TurnEvent::ToolStart` and `TurnEvent::ToolComplete` for each tool call
   - Build updated Conversation with assistant message (text + tool results) appended
   - If tool calls existed, call provider again with results appended (recursive or iterative loop)
   - Emit `TurnEvent::TurnEnd`, return new Conversation
3. On provider stream error: emit `TurnEvent::Error(msg)` and return `Err`
4. Add `pub mod turn;` to `src/lib.rs`
5. Unit tests use `MockProvider` (define inline in test module or import from existing test helpers)
6. DO NOT modify `app.rs`, `main.rs`, or `tui/`

## Dependencies
- Task 01: Core types must be defined in `src/types.rs` first

## Implementation Approach
1. Write failing tests for each turn scenario (TDD RED)
2. Implement `turn()` with the minimal pipeline to make them pass
3. Run full test suite — all 80+ existing tests must still pass

## Acceptance Criteria

1. **Text-only response produces correct events and updated Conversation**
   - Given a MockProvider that returns one TextChunk("Hello") and then TurnEnd
   - When calling `turn(conv.with_user_message("hi"), &provider, &tools, &Middleware::default(), &tx)`
   - Then `TurnEvent::TextChunk("Hello")` and `TurnEvent::TurnEnd` are sent on tx, and the returned Conversation has user + assistant messages appended

2. **TextChunk events arrive in order**
   - Given a MockProvider that returns TextChunk("foo"), TextChunk("bar"), TurnEnd
   - When calling `turn()` and collecting all events
   - Then events arrive in order: TextChunk("foo"), TextChunk("bar"), TurnEnd

3. **Tool call triggers execution and second LLM round**
   - Given a MockProvider that returns a tool_use block followed by a second text response with TurnEnd
   - When calling `turn()` with a ToolRegistry containing a matching tool
   - Then ToolStart and ToolComplete events are emitted, and the Conversation includes both the tool call and result messages

4. **Provider error emits Error event and returns Err**
   - Given a MockProvider that returns `Err(ProviderError::Aws("network failure"))`
   - When calling `turn()`
   - Then `TurnEvent::Error` is sent on tx and the function returns `Err`

5. **Pre-tool Block middleware skips execution**
   - Given a Middleware with one pre_tool closure that returns `Block("not allowed")`
   - When a tool call is triggered by the provider
   - Then the tool is NOT executed, a Block result is returned to the LLM, and ToolComplete event has the block reason

6. **Pre-tool Transform middleware skips execution**
   - Given a Middleware with one pre_tool closure that returns `Transform(ToolResult::ok("mocked result"))`
   - When a tool call is triggered by the provider
   - Then the tool is NOT executed and the mocked result is returned to the LLM

7. **Pre-tool Allow middleware passes through**
   - Given a Middleware with one pre_tool closure that returns `Allow(modified_call)`
   - When a tool call is triggered
   - Then the modified call is used for actual execution

8. **All existing tests pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all 80+ tests pass with zero failures

## Metadata
- **Complexity**: High
- **Labels**: turn, pipeline, fp-refactor
- **Required Skills**: Rust, async/await, tokio
