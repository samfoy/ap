---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Session Persistence for Conversation

## Description

Add `save_conversation` and `load_conversation` methods to `SessionStore` so
that the new `Conversation` type (from `types.rs`) can be persisted to and
restored from disk. This replaces the ad-hoc `Session`→`AgentLoop` wiring that
will be deleted in Step 7.

Keep `Session` and its existing `save`/`load` methods intact — `AgentLoop`
still uses `Session` until Step 7 removes it. This step is purely additive.

## Background

- `src/session/mod.rs` defines `Session` (id, created_at, model, messages)
- `src/session/store.rs` defines `SessionStore` with `save(&Session)` / `load(id) -> Session`
- `src/types.rs` defines `Conversation` (id, model, messages, config) — fully `Serialize`/`Deserialize`
- After this step, `main.rs` (Step 5) can use `SessionStore::save_conversation` / `load_conversation`
  directly, bypassing `Session` entirely

## Reference Documentation

**Required:**
- Design: `ap/src/types.rs` (Conversation struct fields)
- `ap/src/session/store.rs` (existing SessionStore pattern to follow)
- `ap/src/session/mod.rs` (existing Session type — must remain intact)

**Additional References:**
- `.agents/scratchpad/implementation/ap-fp-refactor/progress.md` (overall plan)

**Note:** Read `src/session/store.rs` fully before implementing — follow the same
pattern: store in `<base>/<id>.json`, create dirs on save, descriptive errors on load.

## Technical Requirements

1. Add `save_conversation(&self, conv: &Conversation) -> Result<()>` to `SessionStore`
   - Serialize `conv` as pretty JSON to `<base>/<conv.id>.json`
   - Create parent directories if needed (same as `save`)
2. Add `load_conversation(&self, id: &str) -> Result<Conversation>` to `SessionStore`
   - Read `<base>/<id>.json` and deserialize as `Conversation`
   - Return descriptive `Err` if file missing or malformed
3. Add `use crate::types::Conversation;` to `store.rs` — no other files need to change
4. Keep `Session` and all existing `save`/`load` methods — purely additive change
5. `Conversation`'s `config` field uses `#[serde(default)]` — loading an old session
   written without `config` should not fail (default AppConfig)

## Dependencies

- Step 01 (types.rs) must be complete — `Conversation` must exist ✓
- `src/session/store.rs` + `src/types.rs` are the only files to touch

## Implementation Approach

1. Open `src/session/store.rs` — read the existing `save`/`load` impl
2. Add `use crate::types::Conversation;` to imports
3. Implement `save_conversation` following the same pattern as `save`:
   ```rust
   pub fn save_conversation(&self, conv: &Conversation) -> Result<()> {
       let path = self.path_for(&conv.id);
       if let Some(parent) = path.parent() {
           std::fs::create_dir_all(parent)...
       }
       let json = serde_json::to_string_pretty(conv)...;
       std::fs::write(&path, json)...;
       Ok(())
   }
   ```
4. Implement `load_conversation`:
   ```rust
   pub fn load_conversation(&self, id: &str) -> Result<Conversation> {
       let path = self.path_for(id);
       let contents = std::fs::read_to_string(&path)...;
       let conv: Conversation = serde_json::from_str(&contents)...;
       Ok(conv)
   }
   ```
5. Add 3 unit tests (see ACs below)
6. Run `cargo test` — all tests must pass

## Acceptance Criteria

1. **save_conversation round-trips correctly**
   - Given a `Conversation` with id "test-conv", model "claude", and one user message
   - When `store.save_conversation(&conv)` is called followed by `store.load_conversation("test-conv")`
   - Then the loaded value has the same id, model, and message count as the original

2. **save_conversation creates parent directories**
   - Given a `SessionStore::with_base` pointing to a non-existent nested directory
   - When `save_conversation` is called
   - Then the directory is created and the file is written successfully

3. **load_conversation returns descriptive error for missing file**
   - Given a `SessionStore` with no files
   - When `store.load_conversation("no-such-id")` is called
   - Then the result is `Err` and the error message contains "no-such-id"

4. **Old Session format remains intact**
   - Given the existing `save` / `load` methods for `Session`
   - When `cargo test` is run
   - Then all existing session store tests still pass (no regression)

5. **load_conversation tolerates missing config field**
   - Given a JSON file on disk that contains a serialized `Conversation` without a `config` key
   - When `load_conversation` is called
   - Then deserialization succeeds with `AppConfig::default()`

6. **All tests pass, zero clippy warnings**
   - Given the implementation is complete
   - When `cargo test` and `cargo clippy -- -D warnings` are run
   - Then all tests pass and no warnings are emitted

## Metadata
- **Complexity**: Low
- **Labels**: session, persistence, types
- **Required Skills**: Rust, serde
