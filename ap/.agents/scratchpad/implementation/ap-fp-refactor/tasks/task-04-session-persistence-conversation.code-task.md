---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Update Session Persistence to Use `Conversation`

## Description
Add `save_conversation` and `load_conversation` methods to `SessionStore` so that the `Conversation` type (from `src/types.rs`) can be persisted to disk. The existing `Session`-based API is kept intact for backwards compatibility. `AppConfig` needs `#[serde(default)]` on relevant fields or a `Default` impl to allow loading old session files that lack a `config` field.

## Background
Session persistence currently stores a `Session { id, messages }`. The new `Conversation { id, model, messages, config }` is a superset. We add parallel persistence methods rather than migrating the format, so existing session files continue to work. Old files loaded as `Conversation` will get `AppConfig::default()` for the missing `config` field.

## Reference Documentation
**Required:**
- Design/Plan: ap/.agents/scratchpad/implementation/ap-fp-refactor/plan.md

**Additional References:**
- ap/.agents/scratchpad/implementation/ap-fp-refactor/context.md (codebase patterns)
- ap/src/session/store.rs (existing SessionStore implementation)
- ap/src/session/mod.rs (Session struct)

**Note:** You MUST read the plan document before beginning implementation. Pay attention to the Step 4 section.

## Technical Requirements
1. Ensure `Conversation` in `types.rs` derives `Serialize, Deserialize`
2. Ensure `AppConfig` in `config.rs` has `impl Default` (or derive it) — check what already exists
3. Add `#[serde(default)]` to `Conversation.config` field so old JSON without `config` deserializes cleanly
4. In `src/session/store.rs`, add:
   - `pub fn save_conversation(&self, conv: &Conversation) -> anyhow::Result<()>` — saves to `{base_dir}/{conv.id}.json`
   - `pub fn load_conversation(&self, id: &str) -> anyhow::Result<Conversation>` — loads from `{base_dir}/{id}.json`
5. Unit tests in `src/session/mod.rs` or `src/session/store.rs` covering the 3 new test cases
6. All existing session tests must still pass

## Dependencies
- Task 01: `Conversation` type defined in `src/types.rs`

## Implementation Approach
1. Write failing tests (TDD RED)
2. Implement `save_conversation` / `load_conversation`
3. Ensure `AppConfig::default()` exists (add if missing)
4. Run full suite — all tests pass

## Acceptance Criteria

1. **Conversation save/load roundtrip**
   - Given a `Conversation` with id="test-1", model="claude", one user message, and a non-default AppConfig
   - When calling `store.save_conversation(&conv)` then `store.load_conversation("test-1")`
   - Then the loaded Conversation equals the original (same id, model, messages, config)

2. **Loading old session JSON without config field uses AppConfig::default()**
   - Given a JSON file at `{base_dir}/old-session.json` that contains only `{ "id": "old-session", "model": "claude", "messages": [] }` (no `config` key)
   - When calling `store.load_conversation("old-session")`
   - Then it succeeds and `conv.config` equals `AppConfig::default()`

3. **Conversation id is preserved**
   - Given a Conversation with a specific UUID id
   - When saving and loading
   - Then `conv.id` after load equals the original id

4. **Existing session tests still pass**
   - Given the implementation is complete
   - When running `cargo test`
   - Then all existing session-related tests pass alongside the new ones

## Metadata
- **Complexity**: Low
- **Labels**: session, persistence, fp-refactor
- **Required Skills**: Rust, serde, file I/O
