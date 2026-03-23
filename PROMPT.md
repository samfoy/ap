# PROMPT.md — Session Management UX

## Vision

All `ap` sessions are named and persisted to disk from the first turn. No ephemeral runs. When you start `ap`, it creates a named session automatically. When you run `ap --prompt "..."` non-interactively, it also saves to `~/.ap/sessions/`. The user can resume a previous session with `ap --session <name>` or `ap -s <name>`.

## Requirements

### 1. Auto-name sessions
- On first turn of any session (interactive or headless), generate a session name if none set
- Name format: adjective-noun (e.g. `swift-river`, `bold-pine`) — deterministic from timestamp or random
- Store in `~/.ap/sessions/<name>/` as JSONL conversation file

### 2. Persist `--prompt` mode sessions
- Currently `ap --prompt "..."` discards history after run
- Fix: save to `~/.ap/sessions/<name>/conversation.jsonl` same as interactive mode
- Print session name at end: `Session saved: swift-river`

### 3. Resume sessions
- Add `--session <name>` / `-s <name>` flag to `ap`
- Load conversation history from `~/.ap/sessions/<name>/conversation.jsonl` on startup
- If session not found, print error and exit

### 4. Session store module
- `src/session/store.rs` — functions: `save(name, messages)`, `load(name) -> Vec<Message>`, `list() -> Vec<SessionMeta>`, `generate_name() -> String`
- Sessions dir: `~/.ap/sessions/`
- Each session: `~/.ap/sessions/<name>/conversation.jsonl` (one JSON object per line)

### 5. List sessions
- `ap --list-sessions` prints all saved sessions with name, date, message count
- Format: `swift-river  2026-03-22  14 messages`

## Acceptance Criteria

- `ap --prompt "hello"` saves session to `~/.ap/sessions/<name>/`
- `ap -s swift-river` resumes the session and continues the conversation
- `ap --list-sessions` shows all saved sessions
- `cargo build` passes
- `cargo test` passes (≥204 tests)

Output LOOP_COMPLETE when all acceptance criteria are met and the project builds clean.
