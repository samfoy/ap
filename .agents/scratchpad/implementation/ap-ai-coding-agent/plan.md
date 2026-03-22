# Implementation Plan — ap AI Coding Agent

*Generated from approved design.md. 2026-03-22.*

---

## 1. Test Strategy

### Unit Tests

Each module ships with co-located tests (`#[cfg(test)]` in the same file). Tests are written **before** the implementation within each step.

| Module | Test Cases |
|---|---|
| `tools/read` | `test_read_existing_file` (returns content), `test_read_missing_file` (is_error=true), `test_read_binary_graceful` |
| `tools/write` | `test_write_creates_file`, `test_write_creates_parent_dirs`, `test_write_overwrites_existing` |
| `tools/edit` | `test_edit_replaces_text`, `test_edit_old_text_not_found` (is_error), `test_edit_multiple_matches_error_with_count` |
| `tools/bash` | `test_bash_success_stdout`, `test_bash_nonzero_exit_captured`, `test_bash_stderr_captured`, `test_bash_combined_output` |
| `config` | `test_defaults_when_no_file`, `test_load_project_config`, `test_global_config_merged`, `test_project_overrides_global`, `test_invalid_toml_returns_error` |
| `hooks/runner` | `test_pre_tool_call_proceeds_on_exit_0`, `test_pre_tool_call_cancels_on_nonzero`, `test_post_tool_call_transforms_on_stdout`, `test_post_tool_call_passthrough_on_empty_stdout`, `test_observer_hook_warning_on_nonzero`, `test_hook_script_not_found_warning` |
| `session/store` | `test_save_and_reload_roundtrip`, `test_missing_dir_created`, `test_load_nonexistent_returns_error` |
| `extensions/rhai_loader` | `test_load_valid_rhai_tool`, `test_rhai_execute_returns_result`, `test_rhai_syntax_error_returns_warning`, `test_rhai_missing_function_returns_warning` |
| `extensions/dylib_loader` | `test_missing_symbol_returns_warning` (logic-path test without real dylib) |
| `provider/mod` | `test_stream_event_variants` (enum round-trip), `test_provider_error_display` |

### Integration Tests (`tests/` directory)

- **`tests/agent_loop.rs`**: `MockProvider` that replays a scripted conversation with one tool_use call. Verifies: correct tool dispatched, result batched into user turn, stop on `end_turn`.
- **`tests/hook_cancel.rs`**: pre_tool_call hook (shell script) returns exit 1 → bash tool skipped → synthetic error result present in conversation history.
- **`tests/noninteractive.rs`**: `ap -p "echo hello"` with `MockProvider`, verify stdout contains agent reply, exit 0.

### E2E Test Scenario (Validator executes manually)

**Happy path:**
1. `cd /tmp && mkdir ap-test && cd ap-test && echo "hello world" > test.txt`
2. `ap -p "read test.txt and tell me what it says"` with real Bedrock credentials
3. Expected: Agent reads file, returns summary containing "hello world", exits 0
4. Verify: stdout printed agent response, no crash

**TUI smoke test:**
1. `ap` (no args) in a terminal
2. Press `i`, type "what tools do you have?", press Enter
3. Expected: TUI renders, tool panel shows activity, response appears in conversation pane
4. Press Ctrl+C to quit cleanly

**Adversarial path:**
1. `ap -p "run: sleep 9999"` — verify command runs (no timeout in v1), can be Ctrl+C'd
2. Configure a `pre_tool_call` hook that exits 1 → verify bash tool cancellation message appears
3. Place a `.rhai` file with syntax error in `~/.ap/extensions/` → verify ap starts cleanly with warning, doesn't crash

---

## 2. Implementation Steps

### Step 1: Cargo.toml + Project Scaffold
**Files:** `ap/Cargo.toml`, `ap/src/main.rs`, `ap/ap.toml.example`, `ap/.gitignore`

Set up workspace with all dependencies declared:
```toml
# Key dependencies (full list in step)
ratatui, crossterm, tokio, clap, reqwest, serde, serde_json, toml,
aws-sdk-bedrockruntime, aws-config, aws-credential-types,
futures, anyhow, thiserror, rhai (features = ["sync"]),
libloading, tempfile, dirs
```

`main.rs` parses `--version` via clap and prints it, then exits.

**Tests:** `cargo build --release` succeeds, `ap --version` prints version.
**Demo:** Binary compiles and prints `ap 0.1.0`.

---

### Step 2: Config System
**Files:** `src/config.rs`

- `AppConfig` struct with `ProviderConfig`, `HooksConfig`, `ToolsConfig`
- Load `ap.toml` (project-level) and `~/.ap/config.toml` (global), both optional
- Merge: project overrides global; defaults fill in missing fields
- Return typed error on invalid TOML with file path context

**Tests:** All 5 config unit tests (defaults, load, merge, override, invalid)
**Demo:** `AppConfig::load()` returns defaults when no config file present; prints config in debug mode.

---

### Step 3: Tool Trait + 4 Built-in Tools
**Files:** `src/tools/mod.rs`, `src/tools/read.rs`, `src/tools/write.rs`, `src/tools/edit.rs`, `src/tools/bash.rs`

- `Tool` trait with `BoxFuture` return (object-safe)
- `ToolResult { content: String, is_error: bool }`
- `ToolRegistry { tools: Vec<Box<dyn Tool>> }` with `find_by_name`, `all_schemas`
- 4 built-in tool implementations with JSON schemas
- EditTool: returns error with count when old_text matches >1 occurrence
- BashTool: captures stdout/stderr/exit; format `"{stdout}\n{stderr}\nexit: {code}"`; no timeout

**Tests:** All 12 tool unit tests (3 per tool)
**Demo:** Unit tests pass; can create a ToolRegistry and call `all_schemas()` returning 4 JSON objects.

---

### Step 4: Provider Trait + Bedrock Implementation
**Files:** `src/provider/mod.rs`, `src/provider/bedrock.rs`

- `Provider` trait: `stream_completion` returning `BoxStream<StreamEvent>`
- `StreamEvent` enum: TextDelta, ToolUseStart, ToolUseParams, ToolUseEnd, TurnEnd
- `ProviderError` enum with thiserror
- `BedrockProvider`: wraps `aws_sdk_bedrockruntime::Client`
  - Formats `Vec<Message>` → Bedrock API request body (Anthropic Messages format as JSON)
  - Injects tool schemas into `tool_config`
  - Calls `invoke_model_with_response_stream`
  - Parses streaming chunk JSON → `StreamEvent`s

**Tests:** Provider unit tests for `StreamEvent` variants and `ProviderError` display. Integration path tested later in Step 7.
**Demo:** `BedrockProvider::new()` constructs without panic; struct is wired up correctly (full API test deferred to Step 7 with mock).

---

### Step 5: Hooks System
**Files:** `src/hooks/mod.rs`, `src/hooks/runner.rs`

- `HookRunner` with three entry points: `run_pre_tool_call`, `run_post_tool_call`, `run_observer_hook`
- Env var injection per protocol (Appendix B of design)
- Temp file management for pre_turn/post_turn/on_error (create → hook runs → delete)
- `HookOutcome` enum: Proceed, Cancelled, Transformed, Observed, HookWarning
- Missing/non-executable hook script → non-fatal warning (not an error)

**Tests:** All 6 hook runner tests; test with actual temp shell scripts written in-test.
**Demo:** Unit tests pass; hook runner correctly cancels on exit 1 and transforms on stdout.

---

### Step 6: Extensions System (Rhai + Dylib)
**Files:** `src/extensions/mod.rs`, `src/extensions/loader.rs`, `src/extensions/rhai_loader.rs`, `src/extensions/dylib_loader.rs`

- `Registry` struct: tools (live), hooks/panels/message_interceptors (stubs, collected not executed)
- `Extension`, `Panel`, `MessageInterceptor` traits
- `RhaiTool`: wraps Rhai script as `Box<dyn Tool>`, uses `rhai = { features = ["sync"] }`
- `ExtensionLoader { libraries: Vec<Library> }` with `discover_and_load`
- Extension discovery from `~/.ap/extensions/` and `./.ap/extensions/`
- `Path::extension().and_then(|e| e.to_str())` for correct OsStr matching
- `load_dylib` returns `anyhow::Result<Library>`; Library pushed to `self.libraries`
- Sandbox Rhai engine: disable file I/O and network access

**Tests:** All 5 extension unit tests (Rhai valid load, Rhai execute, Rhai syntax error, Rhai missing fn, dylib missing symbol path).
**Demo:** Place a valid `.rhai` extension in `.ap/extensions/`, run binary, see "Loaded extension: my_tool" in startup log.

---

### Step 7: Agent Loop
**Files:** `src/app.rs`

- `AgentLoop { messages, provider: Arc<dyn Provider>, tools: ToolRegistry, hooks: HookRunner }`
- `run_turn()` method: fires pre_turn → streams LLM → fires post_turn → dispatches tools (sequential) → fires hooks per tool → appends results → loops
- `mpsc::Sender<UiEvent>` for TUI updates (defined here, used in Step 9)
- `UiEvent` enum: TextChunk, ToolStart, ToolComplete, TurnEnd, Error
- `-p` mode path: writes TextChunk events to stdout
- Integration test: `MockProvider` → one full turn with tool_use → verified result

**Tests:** Integration tests in `tests/agent_loop.rs` and `tests/hook_cancel.rs`
**Demo:** `cargo test` passes all agent loop integration tests including tool dispatch and hook cancellation.

---

### Step 8: Session Persistence
**Files:** `src/session/mod.rs`, `src/session/store.rs`

- `Session { id, created_at, model, messages }` with serde derive
- `SessionStore::save(session)` → `~/.ap/sessions/<id>.json` (creates dir if needed)
- `SessionStore::load(id)` → deserializes JSON; non-fatal: returns error if not found
- Auto-generate session ID (uuid or timestamp-based) if not provided
- Wire into AgentLoop: load session on `--session <id>`, autosave after each turn

**Tests:** All 3 session store unit tests
**Demo:** `ap --session test123` starts fresh session, auto-saves; `ap --session test123` again resumes with previous messages.

---

### Step 9: Ratatui TUI
**Files:** `src/tui/mod.rs`, `src/tui/ui.rs`, `src/tui/events.rs`

- `TuiApp` struct: holds terminal, mode state, conversation history buffer, tool events buffer
- Layout: status bar (top), conversation pane (center-left, scrollable), tool panel (center-right), input box (bottom)
- Mode state machine: Normal → Insert (i/Enter) → Normal (Esc)
- `/help` overlay modal with keybindings
- `Ctrl+C` → graceful quit (restore terminal)
- Async integration: `tokio::select!` between crossterm keyboard events and `mpsc::Receiver<UiEvent>` from agent
- Renders incoming `UiEvent`s without blocking the keyboard event poll

**Tests:** No automated TUI tests (ratatui rendering requires a real terminal); manual smoke test is the gate.
**Demo:** `ap` renders TUI without crash, accepts input, displays mock response, `/help` shows keybindings overlay.

---

### Step 10: Non-Interactive Mode (`-p`)
**Files:** `src/main.rs` (dispatch logic)

- `--prompt` / `-p` flag via clap
- When `-p` is present: skip TUI, run agent loop in headless mode
- Stream output directly to stdout (TextChunk events → print to stdout)
- Exit 0 on `end_turn`, exit 1 on error

**Tests:** Integration test `tests/noninteractive.rs` with MockProvider
**Demo:** `ap -p "hello"` with MockProvider returns agent reply to stdout, exits 0. Real Bedrock: `ap -p "read Cargo.toml and summarize it"` works end-to-end.

---

### Step 11: README.md
**Files:** `README.md`

Sections:
- Installation (cargo install from source)
- Quick start (`ap -p "your prompt"`)
- Configuration reference (full `ap.toml` with all keys documented)
- Built-in tools (read, write, edit, bash)
- Hooks system (lifecycle, env vars, examples)
- Extensions: Rhai (example script), Rust dylib (ABI warning, example)
- Session management
- Keybindings reference
- Non-interactive mode

**Demo:** README is complete and accurate; a new user can follow it to get `ap` running.

---

### Step 12: Final Polish
**Files:** All source files

- `cargo clippy -- -D warnings` → zero warnings
- `cargo test` → all tests pass
- `cargo build --release` → clean binary
- Fix all clippy lints
- Review all `unwrap()` calls — replace with proper error handling where appropriate
- Verify TUI terminal cleanup on panic (use `better-panic` or `std::panic::set_hook`)

**Tests:** All tests pass; zero clippy warnings; zero compiler warnings.
**Demo:** All acceptance criteria met:
  - `cargo build --release` succeeds with zero warnings
  - `ap -p "read Cargo.toml and summarize it"` works end-to-end with real Bedrock
  - All 4 tools work with unit tests
  - TUI renders without crashing
  - Hook system executes shell commands at correct lifecycle points
  - Extension discovery loads from `~/.ap/extensions/` without crashing
  - `README.md` is complete and accurate

---

## 3. Wave 1 Tasks (Step 1 + Step 2)

The Task Writer should materialize Steps 1 and 2 as the first implementation wave:
1. Cargo.toml + project scaffold (binary compiles, `ap --version` works)
2. Config system with full unit test coverage

These establish the foundation all subsequent steps build on.

---

## 4. Success Criteria Summary

| Step | Gate | Demo |
|---|---|---|
| 1 | `cargo build --release` succeeds | `ap --version` prints `ap 0.1.0` |
| 2 | Config unit tests pass | `AppConfig::load()` returns defaults |
| 3 | Tool unit tests pass (12 tests) | `ToolRegistry` returns 4 schemas |
| 4 | Provider structs compile + unit tests pass | `BedrockProvider::new()` constructs |
| 5 | Hook unit tests pass (6 tests) | Cancel/transform hooks work |
| 6 | Extension unit tests pass (5 tests) | Valid `.rhai` extension loads |
| 7 | Integration tests pass (agent loop) | Full turn with tool_use verified |
| 8 | Session unit tests pass (3 tests) | Resume session with `--session` |
| 9 | TUI renders, manual smoke test | Keyboard input + `/help` modal |
| 10 | `ap -p` integration test passes | Real Bedrock E2E works |
| 11 | README reviewed for accuracy | New user can follow to completion |
| 12 | Zero warnings, all tests pass | All acceptance criteria met |
