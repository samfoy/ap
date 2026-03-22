# Broken Windows — skill-system

Files that will be touched during implementation. Low-risk smells only.

---

### [provider/mod.rs:84] Missing doc on `Provider` trait method

**Type**: docs
**Risk**: Low
**Fix**: Add `/// Stream a completion given messages, tools, and optional system prompt.`
**Code**:
```rust
pub trait Provider: Send + Sync {
    fn stream_completion<'a>(  // ← no doc comment
        &'a self,
        messages: &'a [Message],
        tools: &'a [serde_json::Value],
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>>;
}
```

---

### [turn.rs:74] Magic number `tool_schemas` variable shadows outer scope

**Type**: naming
**Risk**: Low
**Fix**: Rename to `tool_schemas` is fine — no issue. (False alarm on review.)

---

### [config.rs:52] `overlay_from_table` silently ignores unknown TOML keys

**Type**: docs
**Risk**: Low
**Fix**: Add a comment `// Unknown top-level keys are silently ignored — intentional for forward-compatibility.`

---

### [middleware.rs:69] `pre_turn` hook comment says "observer" but the closure never reads the conversation

**Type**: docs
**Risk**: Low
**Fix**: Clarify comment to: `// pre_turn hook is a pure observer — runs the shell script but never modifies the conversation.`

---

### [turn.rs:43–47] `apply_post_turn` is structurally identical to `apply_pre_turn` — minor duplication

**Type**: duplication
**Risk**: Medium (not low — extracting a shared helper changes both call sites and test coverage)
**Decision**: Do NOT flag as broken window — risk is medium.

---

### [bedrock.rs:92] `build_request_body` lacks a doc comment

**Type**: docs
**Risk**: Low
**Fix**: Add `/// Build the Anthropic Messages API JSON request body.`
