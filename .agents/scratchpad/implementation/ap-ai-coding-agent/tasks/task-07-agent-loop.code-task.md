---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Agent Loop

## Description
Implement `src/app.rs` with `AgentLoop` — the core orchestration engine. It manages conversation state, streams LLM responses, dispatches tool calls, fires hooks, and emits `UiEvent`s. Integration tests use `MockProvider` to verify full turn execution without real Bedrock calls.

## Background
The agent loop is the heart of `ap`. It ties together: Provider (LLM streaming), ToolRegistry (tool dispatch), HookRunner (lifecycle hooks), and `mpsc::Sender<UiEvent>` (TUI or stdout updates). The loop runs until `TurnEnd` is received with no pending tool calls. For `-p` (non-interactive) mode, `UiEvent`s are printed to stdout instead of sent to the TUI.

This step has two integration tests that must pass:
- `tests/agent_loop.rs`: full turn with one tool_use, verified tool dispatched and result appended
- `tests/hook_cancel.rs`: pre_tool_call hook cancels bash tool, synthetic error result in history

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (Step 7, integration tests section)

**Note:** You MUST read the design document before beginning implementation. Section 4.9 (AgentLoop) and 4.10 (UiEvent) are essential.

## Technical Requirements
1. `src/app.rs`:
   - `UiEvent` enum: `TextChunk(String)`, `ToolStart { name: String, params: serde_json::Value }`, `ToolComplete { name: String, result: ToolResult }`, `TurnEnd`, `Error(String)`
   - `AgentLoop` struct:
     - `messages: Vec<Message>`
     - `provider: Arc<dyn Provider>`
     - `tools: ToolRegistry`
     - `hooks: HookRunner`
     - `ui_tx: mpsc::Sender<UiEvent>`
   - `AgentLoop::new(provider, tools, hooks, ui_tx) -> Self`
   - `AgentLoop::run_turn(&mut self, user_input: String) -> anyhow::Result<()>`:
     - Appends user message
     - Fires `pre_turn` observer hook
     - Streams LLM response via `provider.stream_completion()`
     - On `TextDelta`: sends `UiEvent::TextChunk`, accumulates text
     - On `ToolUseStart/Params/End`: collects tool call
     - On `TurnEnd`: fires `post_turn` observer hook
     - For each collected tool call (sequential):
       - Fires `pre_tool_call` hook → if `Cancelled`: append synthetic error result, skip tool, send `UiEvent::ToolComplete` with error
       - If `Proceed`: execute tool, fire `post_tool_call` hook, apply `Transformed` result if applicable
       - Sends `UiEvent::ToolStart` before and `UiEvent::ToolComplete` after
       - Appends tool result to messages
     - If tool calls were made, loop back (call LLM again with results appended)
     - If no tool calls, return `Ok(())`
2. `MockProvider` in `tests/` (or `src/` behind `#[cfg(test)]`):
   - Takes a scripted sequence of `Vec<StreamEvent>` to replay
   - Each call to `stream_completion` yields the next sequence in the script
   - Useful for deterministic integration testing without Bedrock

## Dependencies
- Task 02 (config) — `HooksConfig`
- Task 03 (tool trait) — `ToolRegistry`, `ToolResult`
- Task 04 (provider) — `Provider`, `StreamEvent`, `Message`
- Task 05 (hooks) — `HookRunner`, `HookOutcome`

## Implementation Approach
1. Write integration tests first in `tests/agent_loop.rs` and `tests/hook_cancel.rs` using `MockProvider` (RED)
2. Define `UiEvent` and `AgentLoop` struct
3. Implement `run_turn()` — start with happy path (no tools), then add tool dispatch, then hook integration
4. Run integration tests — iterate until GREEN
5. Ensure `cargo check` is clean

## Acceptance Criteria

1. **Full Turn with Tool Use**
   - Given a `MockProvider` scripted to return one `ToolUseStart/Params/End` for `read` tool, then a final `TextDelta + TurnEnd`
   - When `agent_loop.run_turn("read test.txt".into()).await` is called
   - Then the `read` tool is dispatched, result appended to messages, and final `TurnEnd` UiEvent sent

2. **Hook Cancel Prevents Tool Execution**
   - Given a `pre_tool_call` hook configured to exit 1
   - When the agent loop would dispatch the `bash` tool
   - Then the tool is NOT executed, a synthetic error `ToolResult { is_error: true }` is appended to messages, and the loop continues

3. **TextChunk Events Emitted**
   - Given a `MockProvider` that returns `TextDelta("hello")` + `TurnEnd`
   - When `run_turn` is called
   - Then `UiEvent::TextChunk("hello")` is received on the `ui_rx` channel

4. **Integration Tests Pass**
   - Given the implementation is complete
   - When running `cargo test --test agent_loop && cargo test --test hook_cancel`
   - Then both integration tests pass

5. **No Tool Calls = Single LLM Call**
   - Given a `MockProvider` that returns only `TextDelta + TurnEnd` (no tool use)
   - When `run_turn` is called
   - Then the provider is called exactly once and `TurnEnd` UiEvent is sent

## Metadata
- **Complexity**: High
- **Labels**: agent-loop, integration-test, orchestration, mock
- **Required Skills**: Rust, async, tokio, mpsc channels, integration testing
