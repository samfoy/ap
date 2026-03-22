---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Wire maybe_compress_context in Headless Path

## Description
Wire `maybe_compress_context` into the `run_headless` function in `ap/src/main.rs`. When `config.context.limit` is `Some`, the function should compress the conversation before each `turn()` call. When the limit is `None`, behavior is unchanged.

## Background
The headless path (`run_headless`) builds a `Conversation` with the new user message, then calls `turn()`. After this step, it will: build the conversation with the user message → clone it as `fallback` → call `maybe_compress_context` (if limit is Some) → on `Ok`, use the compressed conversation; on `Err`, log and use `fallback` → call `turn()` with whatever conversation resulted.

**Ownership rule:** `maybe_compress_context` takes ownership. You MUST clone `conv_with_msg` into `fallback` BEFORE calling it. On `Err`, return `fallback` to the pipeline.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/conversation-context-management/design.md`

**Additional References:**
- `.agents/scratchpad/implementation/conversation-context-management/context.md` (codebase patterns)
- `.agents/scratchpad/implementation/conversation-context-management/plan.md` (overall strategy)

**Note:** You MUST read the design document (§4.5 headless path) before beginning implementation. Read `ap/src/main.rs` `run_headless` function in full before making changes.

## Technical Requirements
1. Import `ap::context::maybe_compress_context` and `ap::config::ContextConfig` (if needed)
2. In `run_headless`, after building `conv_with_msg`:
   - `let fallback = conv_with_msg.clone();`
   - `let conv_to_use = if let Some(_limit) = config.context.limit { match maybe_compress_context(conv_with_msg, &config.context, provider.as_ref()).await { Ok((c, Some(evt))) => { log_event_to_stderr(&evt); c }, Ok((c, None)) => c, Err(e) => { eprintln!("warn: context compression failed: {e}"); fallback } } } else { conv_with_msg };`
3. Pass `conv_to_use` to `turn()` instead of `conv_with_msg`
4. No changes to the `None` limit path — tests must see identical behavior

## Dependencies
- Task 05 (`maybe_compress_context` must exist and compile)
- Task 03 (`config.context.limit` overlay from CLI must exist)

## Implementation Approach
1. Read `run_headless` carefully
2. Add the clone + conditional compression block
3. `cargo build` — zero warnings, zero errors
4. `cargo test` — all existing tests pass (behavior is unchanged when limit is None)
5. Manual smoke test: `cargo run -- --context-limit 500 -p "hello"` runs without panic

## Acceptance Criteria

1. **No-Limit Behavior Unchanged**
   - Given `config.context.limit == None`
   - When `run_headless` executes
   - Then `turn()` receives the same conversation as before (no clone overhead on hot path)

2. **Compression Fires When Over Threshold**
   - Given `config.context.limit == Some(small_number)` and a long conversation
   - When `run_headless` executes
   - Then `maybe_compress_context` is called and the compressed conversation is passed to `turn()`

3. **Error Fallback Uses Original Conversation**
   - Given `maybe_compress_context` returns `Err`
   - When `run_headless` executes
   - Then the original `conv_with_msg` (the fallback clone) is passed to `turn()` and an error is logged to stderr

4. **All Existing Tests Pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all pre-existing tests pass and `cargo build` produces zero warnings

5. **Smoke Test**
   - Given the binary is compiled
   - When running `ap --context-limit 500 -p "hello"` (or equivalent)
   - Then the binary runs without panic

## Metadata
- **Complexity**: Medium
- **Labels**: context-management, headless, wiring, main
- **Required Skills**: Rust, async/await, ownership patterns
