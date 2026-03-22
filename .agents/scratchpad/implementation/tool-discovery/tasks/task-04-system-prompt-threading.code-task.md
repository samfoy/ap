---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: System Prompt Threading

## Description
Add `system_prompt: Option<String>` to `Conversation`, update `Provider::stream_completion` to accept `system_prompt: Option<&'a str>`, update `BedrockProvider` to conditionally include `"system"` in the API body, and thread the value through `turn()`. Update all 3 `MockProvider` impl sites to match the new signature.

## Background
Claude's Bedrock API supports a top-level `"system"` field in the request body. Threading `system_prompt` through the pipeline is a pure signature propagation — the compiler will catch all 3 `MockProvider` sites automatically. This step has no new I/O; it's pure type system and signature work.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/tool-discovery/design.md` (Sections 3.3, 3.4, 3.5, 3.6)

**Additional References:**
- `.agents/scratchpad/implementation/tool-discovery/context.md` (codebase patterns)
- `ap/src/types.rs` — current `Conversation` struct
- `ap/src/provider/mod.rs` — current `Provider` trait
- `ap/src/provider/bedrock.rs` — current `BedrockProvider` and `build_request_body`
- `ap/src/turn.rs` — current `turn_loop` call site
- `ap/tests/noninteractive.rs` — integration `MockProvider`
- `.agents/scratchpad/implementation/tool-discovery/plan.md` (Step 4)

**Note:** You MUST read the design document AND all 5 source files listed above before beginning implementation.

## Technical Requirements
1. `ap/src/types.rs` — `Conversation` struct:
   - Add `#[serde(default)] pub system_prompt: Option<String>,`
   - Add builder: `pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self { self.system_prompt = Some(prompt.into()); self }`
2. `ap/src/provider/mod.rs` — `Provider` trait:
   - Update `stream_completion` signature to add `system_prompt: Option<&'a str>` parameter
3. `ap/src/provider/bedrock.rs`:
   - Update `build_request_body` to accept `system_prompt: Option<&str>` and conditionally insert `body["system"] = json!(sp)` when `Some`
   - Update `stream_completion` impl to pass `system_prompt` through
4. `ap/src/turn.rs`:
   - Extract `conv.system_prompt.as_deref()` and pass to `provider.stream_completion`
   - Update inline `MockProvider::stream_completion` signature (adds `_system_prompt: Option<&'a str>`)
5. `ap/tests/noninteractive.rs`:
   - Update `MockProvider::stream_completion` signature (adds `_system_prompt: Option<&'a str>`)
6. No `unwrap()` or `expect()` outside test modules

## Dependencies
- Task 01: `Conversation` type (being modified)
- Tasks 02 and 03 can be done in parallel with this task, but this task must complete before Task 05

## Implementation Approach
1. Write failing unit tests in `types.rs` and `bedrock.rs` (RED):
   - `conversation_with_system_prompt_builder`
   - `conversation_serde_backward_compat`
   - `bedrock_build_request_body_with_system_prompt`
   - `bedrock_build_request_body_no_system_prompt`
2. Update `Conversation` in `types.rs` (GREEN)
3. Update `Provider` trait signature — compiler errors guide remaining changes
4. Update `BedrockProvider` `build_request_body` and `stream_completion`
5. Update `turn.rs` `turn_loop` and inline `MockProvider`
6. Update `tests/noninteractive.rs` `MockProvider`
7. Run `cargo test --package ap` — all tests pass (REFACTOR)

## Acceptance Criteria

1. **Conversation builder sets system_prompt**
   - Given a `Conversation` created with `Conversation::new(...)`
   - When `.with_system_prompt("my prompt")` is chained
   - Then `conv.system_prompt == Some("my prompt".to_string())`

2. **Conversation serde backward compatibility**
   - Given a JSON string of an old `Conversation` without a `"system_prompt"` field
   - When deserialized with `serde_json::from_str`
   - Then deserialization succeeds and `system_prompt == None`

3. **Bedrock request body includes system field when Some**
   - Given `build_request_body(messages, tools, Some("be concise"))`
   - When the resulting JSON is inspected
   - Then `body["system"] == "be concise"`

4. **Bedrock request body omits system field when None**
   - Given `build_request_body(messages, tools, None)`
   - When the resulting JSON is inspected
   - Then the JSON object has no `"system"` key

5. **All MockProvider impls compile with new signature**
   - Given the `Provider` trait signature is updated
   - When `cargo check --package ap` is run
   - Then it compiles cleanly (compiler enforces all 3 impl sites)

6. **All existing tests still pass**
   - Given the signature changes are propagated to all call sites
   - When `cargo test --package ap` is run
   - Then all pre-existing tests pass (regression: zero failures)

7. **Unit Tests Pass**
    - Given the implementation is complete
    - When running `cargo test --package ap`
    - Then all 4 new unit tests pass plus all existing tests

## Metadata
- **Complexity**: Medium
- **Labels**: types, provider, bedrock, threading, rust
- **Required Skills**: Rust, async traits, serde, JSON
