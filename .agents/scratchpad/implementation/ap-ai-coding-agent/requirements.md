# Requirements — ap AI Coding Agent

## Consolidated from Q&A (2026-03-22)

---

## R1: Language & Stack

- **R1.1** Language: Rust (stable toolchain)
- **R1.2** TUI: ratatui + crossterm
- **R1.3** Async runtime: tokio
- **R1.4** CLI: clap (derive API)
- **R1.5** HTTP client: reqwest (async)
- **R1.6** Serialization: serde + serde_json
- **R1.7** Config format: TOML

---

## R2: AI Provider — AWS Bedrock

- **R2.1** Use `aws-sdk-bedrockruntime` (AWS SDK for Rust)
- **R2.2** Default model: `us.anthropic.claude-sonnet-4-6`
- **R2.3** Credentials: standard AWS SDK credential chain (env / `~/.aws/`)
- **R2.4** Region: `us-west-2`
- **R2.5** Support streaming responses via `invoke_model_with_response_stream`
- **R2.6** Tool calls: batch all `tool_result` blocks into a single user turn after tools execute

---

## R3: Built-in Tools

- **R3.1** `read` — read a file, return contents
- **R3.2** `write` — write/create a file (create parent dirs if needed)
- **R3.3** `edit` — replace exact text in a file (old_text → new_text)
- **R3.4** `bash` — run a shell command, return stdout/stderr/exit code
- **R3.5** All 4 tools must have unit tests
- **R3.6** Tools implement a `Tool` trait: `name`, `description`, `schema` (JSON Schema), `execute`

---

## R4: Tool Execution Model

- **R4.1** v1: **sequential execution** — tools run one at a time, in the order Claude emitted them
- **R4.2** Same sequential behavior in both TUI and `-p` (non-interactive) modes
- **R4.3** If a `pre_tool_call` hook cancels one tool, remaining tools in the batch still run
- **R4.4** All tool results (including cancelled ones as errors) are batched into a single user turn
- **R4.5** Document `[tools] parallel = false` config key for future v2 parallel support (do not implement)

---

## R5: Hooks System

### R5.1 Hook Types

| Hook | Trigger | Modifies? |
|------|---------|-----------|
| `pre_tool_call` | Before any tool executes | Yes — can cancel (exit code gate) |
| `post_tool_call` | After tool executes | Yes — stdout replaces result content |
| `pre_turn` | Before messages sent to LLM | No — read-only observer |
| `post_turn` | After assistant response received | No — read-only observer |
| `on_error` | On any agent error | No — read-only observer |

### R5.2 pre_tool_call Protocol

- **R5.2.1** Exit code 0 = proceed; non-zero = cancel this tool call
- **R5.2.2** Hook stdout → cancellation reason text (fed to synthetic tool_result)
- **R5.2.3** Hook stderr → logged to TUI tool panel only (never sent to Claude)
- **R5.2.4** If stdout is empty and exit code != 0, fallback: `"Tool call cancelled by pre_tool_call hook (exit code N)"`
- **R5.2.5** Cancelled tool → synthetic `tool_result` with `is_error: true` and cancellation reason text
- **R5.2.6** Env vars: `AP_TOOL_NAME` (string), `AP_TOOL_PARAMS` (JSON string)

### R5.3 post_tool_call Protocol

- **R5.3.1** Non-empty stdout → replaces tool_result content sent to Claude
- **R5.3.2** Empty stdout → original tool result forwarded unchanged
- **R5.3.3** Non-zero exit → advisory warning only; original result forwarded (hook failure never blocks loop)
- **R5.3.4** Env vars: `AP_TOOL_NAME`, `AP_TOOL_PARAMS`, `AP_TOOL_RESULT` (JSON), `AP_TOOL_IS_ERROR` (true/false)

### R5.4 pre_turn / post_turn / on_error Protocol

- **R5.4.1** All three are read-only observers in v1
- **R5.4.2** Hook stdout is ignored; non-zero exit = advisory warning only
- **R5.4.3** Large payloads (message arrays, responses) delivered via temp file paths in env vars
- **R5.4.4** Temp files created before hook runs, deleted after hook exits
- **R5.4.5** `pre_turn` env vars: `AP_HOOK_TYPE`, `AP_TURN_NUMBER`, `AP_SESSION_ID`, `AP_MODEL`, `AP_MESSAGES_FILE`
- **R5.4.6** `post_turn` env vars: `AP_HOOK_TYPE`, `AP_TURN_NUMBER`, `AP_SESSION_ID`, `AP_MODEL`, `AP_RESPONSE_FILE`, `AP_HAS_TOOL_USE`
- **R5.4.7** `on_error` follows pre/post_turn pattern (read-only observer, temp file for error context)

### R5.5 Hook Configuration

- Hooks configured in `ap.toml` under `[hooks]` section
- One hook command per lifecycle point (string path to executable)

---

## R6: Extensions System

- **R6.1** Extension discovery from `~/.ap/extensions/` and `./.ap/extensions/` at startup
- **R6.2** Extensions implement `Extension` trait: `name`, `version`, `register(&mut Registry)`
- **R6.3** `Registry` allows: register tools, register hooks, add TUI panels, intercept messages
- **R6.4** v1: interface-only (trait defined, no actual dylib loading) — API must be correct
- **R6.5** Extension system must not crash when directories don't exist

---

## R7: TUI

- **R7.1** Top: status bar (model, provider, token count)
- **R7.2** Center-left: conversation/agent output (scrollable)
- **R7.3** Center-right: tool activity / live tool output
- **R7.4** Bottom: input box (multiline, vim-ish keybindings)
- **R7.5** Overlay: help modal (at minimum); file picker optional
- **R7.6** Key bindings: `i`/`Enter` focus input, `Esc` normal mode, `Ctrl+C` quit, `Ctrl+L` clear, `/help` keybindings
- **R7.7** TUI must render without crashing

---

## R8: Non-interactive Mode

- **R8.1** `ap -p "prompt"` — run headless, stream output to stdout, exit
- **R8.2** Same tool execution semantics as TUI mode
- **R8.3** Good for scripting and being driven by orchestrators (e.g., Ralph)

---

## R9: Session Persistence

- **R9.1** Sessions saved to `~/.ap/sessions/<id>.json`
- **R9.2** `--session <id>` flag to resume a saved session
- **R9.3** Session format: JSON (conversation history + metadata)

---

## R10: Configuration

- **R10.1** Project-level: `ap.toml` (current directory)
- **R10.2** Global: `~/.ap/config.toml`
- **R10.3** Project config merges over / overrides global config
- **R10.4** Provide `ap.toml.example` in repo

---

## R11: Build & Quality

- **R11.1** `cargo build --release` with zero warnings
- **R11.2** `cargo clippy` clean
- **R11.3** `cargo test` passes
- **R11.4** `README.md` complete and accurate

---

## R12: Acceptance Criteria

1. `cargo build --release` succeeds with zero warnings
2. `ap -p "read Cargo.toml and summarize it"` works end-to-end with real Bedrock calls
3. All 4 tools work and have unit tests
4. TUI renders without crashing
5. Hook system executes shell commands at correct lifecycle points
6. Extension discovery loads from `~/.ap/extensions/` without crashing
7. `README.md` is complete and accurate
