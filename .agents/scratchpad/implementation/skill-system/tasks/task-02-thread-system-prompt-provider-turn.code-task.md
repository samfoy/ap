---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Thread system_prompt through Provider trait and turn()

## Description
Extend the `Provider` trait with a `system_prompt: Option<&str>` parameter on `stream_completion`, update `BedrockProvider` to inject it into the Bedrock JSON body as `"system"`, and update `turn()` to read `conv.system_prompt.as_deref()` and pass it through. Update all provider implementations including test mocks.

## Background
`system_prompt` on `Conversation` is set by the skill injection middleware. The `turn()` function must thread this value to the provider so Bedrock receives it as the `"system"` field. This is the most invasive change (trait signature) and is done early to minimize downstream breakage accumulation.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/skill-system/design.md (Section 2 FR-2, Section 4.2, Appendix B.1)

**Additional References:**
- .agents/scratchpad/implementation/skill-system/context.md (Provider trait shape, MockProvider/ErrorProvider)
- .agents/scratchpad/implementation/skill-system/plan.md (Step 2)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Add `system_prompt: Option<&'a str>` as a new parameter to `Provider::stream_completion` (add lifetime `'a` to trait if not already present)
2. Update `BedrockProvider::stream_completion` to accept and forward the parameter to `build_request_body`
3. Update `build_request_body` to insert `"system": text` into the JSON body when `Some`, omit the key when `None`
4. Update `turn()` to call `provider.stream_completion(..., conv.system_prompt.as_deref())`
5. Update `MockProvider` and `ErrorProvider` in `src/turn.rs` test module (both must have `_system_prompt: Option<&str>` param)
6. All existing tests must continue to pass; `cargo build` succeeds

## Dependencies
- Task 01 (Step 1): `Conversation` must have `system_prompt` field

## Implementation Approach
1. **RED**: Write two failing tests in `src/provider/bedrock.rs`:
   - `provider_passes_system_prompt_to_bedrock`: call `build_request_body([], [], Some("text"))`, assert JSON body contains `"system": "text"`
   - `provider_no_system_prompt_omits_field`: call `build_request_body([], [], None)`, assert JSON body has no `"system"` key
2. **GREEN**: Update trait + all impls in dependency order (trait → BedrockProvider → turn() → test mocks)
3. **REFACTOR**: `cargo clippy --all-targets -- -D warnings` clean

## Acceptance Criteria

1. **System prompt injected when Some**
   - Given `build_request_body` called with `system_prompt = Some("be helpful")`
   - When the resulting JSON is parsed
   - Then it contains `"system": "be helpful"`

2. **System prompt omitted when None**
   - Given `build_request_body` called with `system_prompt = None`
   - When the resulting JSON is parsed
   - Then it has no `"system"` key

3. **turn() threads the value**
   - Given a `Conversation` with `system_prompt = Some("text")`
   - When `turn()` is called
   - Then the provider receives `Some("text")` as its `system_prompt` argument

4. **Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test -- provider`
   - Then both Bedrock tests pass

5. **Full suite still green**
   - Given the implementation is complete
   - When running `cargo test`
   - Then 0 failures (existing turn.rs tests unaffected)

## Metadata
- **Complexity**: Medium
- **Labels**: provider, bedrock, turn, trait
- **Required Skills**: Rust, async, serde_json
