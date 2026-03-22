# Idea Honing — conversation-context-management

## Q1

**Question:** Should the implementation add `Conversation::with_messages(Vec<Message>) -> Self` (functional builder) to support `maybe_compress_context`, or is a local `mut` binding on a cloned struct acceptable per AGENTS.md rules?

**Research:**

The `Conversation` impl already has two consuming builders:

```rust
pub fn with_user_message(mut self, content: impl Into<String>) -> Self { … }
pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self { … }
```

Both use `mut self` as a local rebinding inside the method body — not an external `let mut` accumulation. The call site pattern is:

```rust
let conv2 = conv.clone().with_user_message("hello");
```

**Answer:** **Add `Conversation::with_messages(Vec<Message>) -> Self`.**

Rationale:

1. **Consistency with existing pattern.** The codebase already uses consuming builders with `mut self` for all `Conversation` transformations. `with_messages` is the same idiom applied to a different field.

2. **The `mut` AGENTS.md warns against is call-site `let mut`.** `mut self` inside a method body is an implementation detail — the method's interface is pure: consume input, return transformed value. The linter rule is about observable mutation (accumulation, state-over-time), not internal rebinding in a consuming function.

3. **The alternative is worse.** `let mut new_conv = conv.clone(); new_conv.messages = new_messages; new_conv` at every call site is exactly the imperative pattern AGENTS.md flags: a `let mut` binding that's modified then returned. That's a red flag; `conv.clone().with_messages(new_messages)` is not.

4. **Small, isolated change.** Adding one 4-line builder to `Conversation` has zero ripple effect and makes `maybe_compress_context` read as a clean functional pipeline.

**Decision:** Add `with_messages` to `Conversation`. Confidence: 95.

## Requirements Status: COMPLETE

All gaps resolved. The PROMPT.md spec is self-contained with:
- Exact Rust signatures for all new public items
- 7 independently-compilable ordered steps
- 12 binary acceptance criteria
- Token estimation heuristic fully specified (chars/4, min 1)
- `find_summary_split` algorithm fully specified
- `maybe_compress_context` 10-step algorithm fully specified
- TUI changes fully specified
- Required tests enumerated per step
- The only ambiguity (with_messages builder) is now resolved

Ready for Architect hat.
