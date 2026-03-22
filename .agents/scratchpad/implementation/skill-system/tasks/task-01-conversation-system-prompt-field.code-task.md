---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Extend Conversation with system_prompt field and builder

## Description
Add a transient `system_prompt: Option<String>` field to the `Conversation` struct in `src/types.rs`. The field must NOT be persisted to session JSON (`#[serde(skip)]`). Add a `with_system_prompt()` builder method.

## Background
The skill injection middleware will set a system prompt on the `Conversation` before each turn. Using `#[serde(skip)]` ensures that stale TF-IDF results from prior sessions never leak into new sessions — each session starts fresh with `None`.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/skill-system/design.md (Section 2, FR-1)

**Additional References:**
- .agents/scratchpad/implementation/skill-system/context.md (Conversation builder pattern)
- .agents/scratchpad/implementation/skill-system/plan.md (Step 1)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. Add `#[serde(skip)] pub system_prompt: Option<String>` to the `Conversation` struct
2. Add `pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self` builder method
3. The field must default to `None` (ensured by `#[serde(skip)]` + `Default`)
4. All existing `Conversation` tests must continue to pass
5. No clippy warnings (`#[allow]` must not be needed)

## Dependencies
- None (first step)

## Implementation Approach
1. **RED**: Write two failing tests in `src/types.rs`:
   - `conversation_system_prompt_not_serialized`: serialize a `Conversation` with `system_prompt = Some("text")`, deserialize it, assert `system_prompt` is `None`
   - `conversation_with_system_prompt_builder`: call `.with_system_prompt("hello")`, assert field equals `Some("hello".to_string())`
2. **GREEN**: Add the field + builder to make tests pass
3. **REFACTOR**: Ensure `Default` impl (if manual) is updated; `cargo clippy` clean

## Acceptance Criteria

1. **Field is transient**
   - Given a `Conversation` with `system_prompt = Some("text")`
   - When serialized to JSON and deserialized back
   - Then the deserialized `Conversation` has `system_prompt == None`

2. **Builder sets the field**
   - Given a fresh `Conversation`
   - When `.with_system_prompt("hello")` is called
   - Then `conv.system_prompt == Some("hello".to_string())`

3. **Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test conversation_system_prompt`
   - Then both tests pass

4. **Existing tests unaffected**
   - Given existing `types.rs` tests
   - When running `cargo test`
   - Then all pass with 0 failures

## Metadata
- **Complexity**: Low
- **Labels**: types, conversation, serde
- **Required Skills**: Rust, serde
