---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Session Persistence

## Description
Implement `src/session/mod.rs` and `src/session/store.rs` with save/load JSON session functionality. Sessions persist conversation history to `~/.ap/sessions/<id>.json`. The agent loop loads existing sessions on `--session <id>` and autosaves after each turn.

## Background
Session persistence lets users resume conversations. The session file format is straightforward JSON. The important behaviors are: auto-create the sessions directory if it doesn't exist, return a typed error on load failure (not a panic), and generate a unique session ID when `--session` is not specified.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (Step 8 and session test table)

**Note:** You MUST read the design document before beginning implementation. Section 4.11 covers session persistence.

## Technical Requirements
1. `src/session/mod.rs`:
   - `Session` struct: `id: String`, `created_at: String` (ISO 8601), `model: String`, `messages: Vec<Message>` — derives Serialize/Deserialize/Debug/Clone
   - `Session::new(id: String, model: String) -> Self` — sets `created_at` to current UTC time, empty messages
2. `src/session/store.rs` — `SessionStore`:
   - `SessionStore::save(session: &Session) -> anyhow::Result<()>`:
     - Path: `~/.ap/sessions/<id>.json`
     - Creates `~/.ap/sessions/` directory if it doesn't exist
     - Serializes to pretty-printed JSON and writes
   - `SessionStore::load(id: &str) -> anyhow::Result<Session>`:
     - Path: `~/.ap/sessions/<id>.json`
     - Returns `Err` (not panic) if file doesn't exist or is malformed JSON
     - Error message includes the path that was tried
3. Session ID generation: if `--session` not provided, generate using `uuid::Uuid::new_v4().to_string()`
4. Wire into `AgentLoop`: accept `Option<Session>`, autosave after each turn via `SessionStore::save`

## Dependencies
- Task 01 (project scaffold) — `uuid`, `dirs`, `serde_json` declared
- Task 04 (provider) — `Message` type in `Session.messages`
- Task 07 (agent loop) — agent loop wires in session persistence

## Implementation Approach
1. Write all 3 unit tests (RED):
   - `test_save_and_reload_roundtrip` — save a Session, load it back, verify fields match
   - `test_missing_dir_created` — save to a path where sessions dir doesn't exist, verify dir created
   - `test_load_nonexistent_returns_error` — load a session ID that doesn't exist → returns Err with path
2. Implement `Session` struct and `SessionStore`
3. Wire `SessionStore` into `AgentLoop` — load on init if session ID provided, save after each turn
4. Run tests

## Acceptance Criteria

1. **Save and Reload Roundtrip**
   - Given a `Session` with id `"test-session"`, model `"claude"`, and one message
   - When `SessionStore::save(&session)` then `SessionStore::load("test-session")` are called
   - Then the loaded session has the same id, model, and message count

2. **Missing Directory Auto-Created**
   - Given `~/.ap/sessions/` does not exist
   - When `SessionStore::save(&session)` is called
   - Then the directory is created and the file is written successfully

3. **Load Nonexistent Returns Error With Path**
   - Given no session file for id `"nonexistent-xyz"`
   - When `SessionStore::load("nonexistent-xyz")` is called
   - Then returns `Err(...)` and the error message contains `"nonexistent-xyz"` or the file path

4. **All 3 Session Tests Pass**
   - Given the implementation is complete
   - When running `cargo test session`
   - Then all 3 session tests pass

5. **`--session` Flag Works**
   - Given `ap --session my-session` is run
   - When the session file exists
   - Then it loads the previous messages (demonstrated by build compilation + flag parsing)

## Metadata
- **Complexity**: Low
- **Labels**: session, persistence, serde, json
- **Required Skills**: Rust, serde_json, file I/O, dirs crate
