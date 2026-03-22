---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Non-Interactive Mode (`-p`)

## Description
Wire up the `-p`/`--prompt` flag in `src/main.rs` to run the agent loop headlessly, streaming output to stdout and exiting with code 0 on success or 1 on error. Integration test with `MockProvider` verifies the full path.

## Background
Non-interactive mode is how `ap` is used from scripts, CI, and Ralph itself. When `-p` is provided, no TUI is initialized — the agent loop runs and `UiEvent::TextChunk` events are printed directly to stdout. This mode uses the same `AgentLoop` as the TUI path but with a stdout sink instead of an `mpsc::Sender<UiEvent>`.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (Step 10 and `tests/noninteractive.rs`)

**Note:** You MUST read the design document before beginning implementation. Section 4.13 covers non-interactive mode.

## Technical Requirements
1. `src/main.rs` clap struct:
   - `#[arg(short = 'p', long = "prompt")]` → `Option<String>`
   - `#[arg(long = "session")]` → `Option<String>`
   - `#[arg(long = "model")]` → `Option<String>` (overrides config)
   - When `--prompt` is present: run headless mode
   - When `--prompt` is absent: run TUI mode
2. Headless mode dispatch:
   - Load config, initialize `ToolRegistry::with_defaults()`, `HookRunner`, `BedrockProvider` (or injected provider)
   - Create `mpsc::channel::<UiEvent>()` 
   - Spawn `AgentLoop::run_turn(prompt)` in background
   - In foreground: receive `UiEvent`s from channel, print `TextChunk` to stdout (flush after each chunk), ignore `ToolStart`/`ToolComplete` (or print to stderr), exit on `TurnEnd`
   - Exit code: 0 on `TurnEnd`, 1 on `UiEvent::Error`
3. Integration test `tests/noninteractive.rs`:
   - Use `MockProvider` scripted to return one `TextDelta("Hello from mock") + TurnEnd`
   - Invoke headless mode programmatically (not via subprocess)
   - Verify the output channel received `TextChunk("Hello from mock")` and `TurnEnd`

## Dependencies
- Task 01 (project scaffold) — clap derive declared
- Task 02 (config) — `AppConfig::load()`
- Task 03 (tool trait) — `ToolRegistry`
- Task 05 (hooks) — `HookRunner`
- Task 07 (agent loop) — `AgentLoop`, `UiEvent`
- Task 08 (session) — `SessionStore`

## Implementation Approach
1. Write integration test `tests/noninteractive.rs` using `MockProvider` (RED)
2. Add clap args to `main.rs`
3. Implement headless dispatch function (extract to `src/headless.rs` if it becomes large)
4. Wire real dependencies in `main()`
5. Run integration test (GREEN)
6. Manual E2E test: `ap -p "read Cargo.toml and summarize it"` with real Bedrock

## Acceptance Criteria

1. **Integration Test Passes**
   - Given `MockProvider` scripted with `TextDelta("Hello from mock") + TurnEnd`
   - When the headless mode is invoked with prompt `"test"`
   - Then the `UiEvent::TextChunk("Hello from mock")` is received and `TurnEnd` signals completion

2. **Exit Code 0 on Success**
   - Given a successful run with `MockProvider`
   - When headless mode completes normally
   - Then the process exits with code 0

3. **Exit Code 1 on Error**
   - Given a `MockProvider` that returns `UiEvent::Error("something failed")`
   - When headless mode processes the error
   - Then the process exits with code 1

4. **`-p` Flag Parsed Correctly**
   - Given `ap -p "hello world"`
   - When clap parses args
   - Then headless mode is activated with prompt `"hello world"`, TUI is not initialized

5. **Real Bedrock E2E Works (Manual)**
   - Given valid AWS credentials and `ap -p "read Cargo.toml and summarize it"`
   - When run in the `ap/` directory
   - Then the agent reads `Cargo.toml`, prints a summary to stdout, and exits 0

6. **`tests/noninteractive.rs` Integration Test Passes**
   - Given the implementation is complete
   - When running `cargo test --test noninteractive`
   - Then the test passes

## Metadata
- **Complexity**: Medium
- **Labels**: cli, non-interactive, headless, integration-test
- **Required Skills**: Rust, clap, tokio, process exit codes
