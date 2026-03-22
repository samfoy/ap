# Broken Windows — conversation-context-management

## Touched Files Review

Files that will be modified in this feature:
- `ap/src/types.rs` — add `with_messages`, `TurnEvent::ContextSummarized`
- `ap/src/config.rs` — add `ContextConfig`, overlay in `overlay_from_table`
- `ap/src/main.rs` — add `--context-limit` CLI arg, wire headless path
- `ap/src/tui/mod.rs` — new fields, handle new events, `headless_with_limit`
- `ap/src/tui/ui.rs` — `render_status_bar` update
- `ap/src/lib.rs` — add `context` module

### [ap/src/tui/mod.rs:413-415] Usage event accumulates but does not track current context size
**Type**: dead-code (existing accumulation is still used for cost calculation — not dead code; actually needed)
**Risk**: N/A — this is addressed by the feature itself (adding `last_input_tokens`)

### [ap/src/main.rs:162-163] route_headless_events ignores ContextSummarized
**Type**: dead-code (future)
**Risk**: Low
**Fix**: After `TurnEvent::ContextSummarized` is added in Step 4, `route_headless_events` needs a match arm.
Without it, Rust will emit a non-exhaustive match warning (or error under `#[deny]`). This is not a
broken window but a required change. Note it here for the Builder.

### [ap/src/main.rs:165] route_headless_events match ignores ToolComplete and Usage via comment
**Type**: docs
**Risk**: Low
**Fix**: The `ContextSummarized` variant should log to stderr in headless mode (summary occurred). The
existing comment at line 165-167 explains why `ToolComplete` and `Usage` are ignored — same pattern
can be followed for `ContextSummarized` with a log message.

### [ap/src/config.rs:74] SkillsConfig not Serialize/Deserialize — inconsistency
**Type**: naming/docs
**Risk**: Low (don't fix — it's intentional: SkillsConfig is transient)
**Note**: `ContextConfig` SHOULD be `Serialize/Deserialize` with `#[serde(default)]` since
`limit` should persist with session context. The asymmetry is intentional.

### No other broken windows identified in touched files.
All touched files are well-structured, consistent, and free of dead code or obvious
formatting issues.
