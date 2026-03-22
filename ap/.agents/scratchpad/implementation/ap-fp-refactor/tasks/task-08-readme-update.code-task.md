---
status: pending
created: 2026-03-22
started: null
completed: null
---
# Task: README Update — Document Middleware API and New Architecture

## Description
Update `ap/README.md` to document the new FP-oriented architecture. Add an **Architecture** section describing the `turn()` pipeline and immutable `Conversation`. Add an **Extending ap** section showing how to add middleware closures. Update all references that mention `AgentLoop` or `UiEvent`. Remove any references to the old hook system being the primary extension mechanism (shell hooks are now a backward-compat adapter).

## Background
The README accurately described the old architecture (AgentLoop, shell hooks as primary extension). After the FP refactor, the primary extension mechanism is the Rust `Middleware` chain. Shell hooks still work via the bridge adapter. The README needs to reflect this accurately.

## Reference Documentation
**Required:**
- Design/Plan: ap/.agents/scratchpad/implementation/ap-fp-refactor/plan.md

**Additional References:**
- ap/README.md (current README to update)
- ap/src/middleware.rs (for accurate Middleware API docs)
- ap/src/types.rs (for accurate type signatures)

**Note:** You MUST read the plan document AND the current README before beginning implementation.

## Technical Requirements
1. Add **Architecture** section:
   - Describe the `turn()` pipeline: pre_turn → stream_completion → collect_tool_calls → execute_tools (with middleware) → append_turn
   - Describe `Conversation` immutability: each turn returns a new Conversation
   - Brief code example showing the `turn()` call
2. Add **Extending ap** section:
   - Show how to add a pre_tool logging middleware closure (code example)
   - Show how to Block a tool call with a pre_tool closure
   - Show how to Transform a tool result with a post_tool closure
3. Add/update **Middleware** section:
   - Table: chain name | hook type | signature | behavior
   - `pre_turn` | Fn(&Conversation) → Option<Conversation>
   - `post_turn` | Fn(&Conversation) → Option<Conversation>
   - `pre_tool` | Fn(ToolCall) → ToolMiddlewareResult | Allow/Block/Transform
   - `post_tool` | Fn(ToolCall) → ToolMiddlewareResult
4. Update "Hooks System" section: clarify shell hooks work via `shell_hook_bridge()` adapter for backwards compat
5. Remove any references to `AgentLoop`, `UiEvent`, or the hooks system being the only extension mechanism
6. Verify: `grep -i "agentloop\|uievent" ap/README.md` → zero matches

## Dependencies
- Task 07: app.rs deleted — README must not reference AgentLoop

## Implementation Approach
1. Read current README
2. Add Architecture section
3. Add/update Extending ap and Middleware sections
4. Update Hooks section
5. grep-verify no stale references

## Acceptance Criteria

1. **Architecture section is present and accurate**
   - Given the updated README
   - When reading the Architecture section
   - Then it describes the turn() pipeline steps accurately and matches the implementation in turn.rs

2. **Extending ap section shows middleware usage**
   - Given the Extending ap section
   - When reading the pre_tool logging example
   - Then the code compiles (types match the actual Middleware API)

3. **Middleware table is complete**
   - Given the Middleware section
   - When checking the table
   - Then all four chains (pre_turn, post_turn, pre_tool, post_tool) are documented with correct signatures

4. **No stale references to AgentLoop or UiEvent**
   - Given the updated README
   - When running `grep -i "agentloop\|uievent" ap/README.md`
   - Then zero matches are returned

5. **Shell hooks section updated**
   - Given the Hooks System section
   - When reading it
   - Then it clarifies that shell hooks work via the bridge adapter for backwards compatibility, and that the primary extension mechanism is the Rust Middleware chain

## Metadata
- **Complexity**: Low
- **Labels**: readme, docs, fp-refactor
- **Required Skills**: Technical writing, Rust
