---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: Provider Trait + Bedrock Implementation

## Description
Implement the `Provider` trait in `src/provider/mod.rs` and the `BedrockProvider` in `src/provider/bedrock.rs`. The provider streams LLM responses as `StreamEvent`s. Unit tests cover the enum variants and error display; full API integration is tested later in the agent loop step.

## Background
The `Provider` trait is the abstraction that lets the agent loop work with both real Bedrock and a `MockProvider` in tests. Getting the trait right here means all integration tests can be written without real AWS credentials. The Bedrock implementation uses the legacy `invoke_model_with_response_stream` API (not `converse_stream`) per spec. Internally it formats the Anthropic Messages API as JSON and parses streaming chunks.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (Step 4)

**Note:** You MUST read the design document before beginning implementation. Section 4.4 defines the Provider trait, StreamEvent, and ProviderError. Section 4.6 covers the Bedrock implementation details.

## Technical Requirements
1. `src/provider/mod.rs`:
   - `StreamEvent` enum: `TextDelta(String)`, `ToolUseStart { id: String, name: String }`, `ToolUseParams(String)` (JSON fragment), `ToolUseEnd`, `TurnEnd`
   - `ProviderError` enum with `thiserror`: `Aws(String)`, `ParseError(String)`, `Serialization(#[from] serde_json::Error)`
   - `Provider` trait: `fn stream_completion<'a>(&'a self, messages: &'a [Message], tools: &'a [serde_json::Value]) -> BoxStream<'a, Result<StreamEvent, ProviderError>>` — object-safe via `BoxStream`
   - `Message { role: Role, content: MessageContent }` — serializable
   - `Role` enum: `User`, `Assistant`
   - `MessageContent` enum: `Text(String)`, `ToolUse { id, name, input: serde_json::Value }`, `ToolResult { tool_use_id: String, content: String, is_error: bool }`
2. `src/provider/bedrock.rs`:
   - `BedrockProvider { client: aws_sdk_bedrockruntime::Client, model: String }`
   - `BedrockProvider::new(model: String, region: String) -> anyhow::Result<Self>` — loads credentials from environment/`~/.aws/`
   - Implements `Provider` trait
   - Formats `messages` + `tools` → Anthropic Messages API JSON body
   - Calls `invoke_model_with_response_stream`
   - Parses streaming response chunks → `StreamEvent`s (handle `content_block_start`, `content_block_delta`, `message_stop` event types)
3. Unit tests in `src/provider/mod.rs`:
   - `test_stream_event_variants` — construct each `StreamEvent` variant, verify debug/display
   - `test_provider_error_display` — verify `ProviderError::Aws("msg".into())` formats correctly

## Dependencies
- Task 01 (project scaffold) — AWS SDK crates, `futures`, `thiserror` declared
- Task 03 (Tool trait) — `ToolResult` type used in `MessageContent::ToolResult`

## Implementation Approach
1. Define all types in `mod.rs` first
2. Write the 2 unit tests (RED)
3. Implement types to make tests pass (GREEN)
4. Implement `BedrockProvider::new()` — verify it compiles (no panic, but no API call)
5. Implement `Provider` for `BedrockProvider` — the streaming parser is the complex part; write it carefully referencing the Bedrock streaming response format

## Acceptance Criteria

1. **StreamEvent Variants Construct**
   - Given the `StreamEvent` enum
   - When constructing each variant (`TextDelta`, `ToolUseStart`, `ToolUseParams`, `ToolUseEnd`, `TurnEnd`)
   - Then all variants compile and are debuggable

2. **ProviderError Displays Correctly**
   - Given `ProviderError::Aws("connection failed".into())`
   - When calling `.to_string()`
   - Then the output contains "connection failed"

3. **BedrockProvider Constructs Without Panic**
   - Given valid `model` and `region` strings
   - When calling `BedrockProvider::new(model, region).await`
   - Then returns `Ok(BedrockProvider)` without panic (credentials may not be valid in CI — construction must not validate credentials eagerly)

4. **Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test provider`
   - Then all provider unit tests pass

5. **Compiles Clean**
   - Given the full project
   - When running `cargo check`
   - Then zero errors and zero warnings

## Metadata
- **Complexity**: High
- **Labels**: provider, bedrock, aws, streaming, async
- **Required Skills**: Rust, async streams, AWS SDK, Anthropic Messages API format
