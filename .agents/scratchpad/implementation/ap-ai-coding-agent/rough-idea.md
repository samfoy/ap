# ap — AI Coding Agent in Rust

## Vision

`ap` is a first-class, extensible AI coding assistant that runs in the terminal. It should feel like a native tool — fast, composable, and hackable. Think of it as a spiritual sibling to `pi`, but in Rust, with a ratatui UI and a clean extension system baked in from day one.

## Core Requirements

### Language & Stack
- **Language:** Rust (stable toolchain)
- **TUI:** ratatui + crossterm
- **Async:** tokio
- **CLI:** clap (derive API)
- **HTTP client:** reqwest (async)
- **Serialization:** serde + serde_json
- **Config:** toml (config files)

### AI Provider: AWS Bedrock
- Use AWS SDK for Rust (`aws-sdk-bedrockruntime`)
- Default model: `us.anthropic.claude-sonnet-4-6`
- Credentials: pick up from environment / `~/.aws/` (standard AWS SDK credential chain)
- Support streaming responses (invoke_model_with_response_stream)
- Region: us-west-2

### Built-in Tools (first-class, always available)
1. **read** — read a file, return contents
2. **write** — write/create a file
3. **edit** — replace text in a file (old_text → new_text)
4. **bash** — run a shell command, return stdout/stderr/exit code

### Hooks System
First-class lifecycle hooks, configurable via `ap.toml`:
- `pre_tool_call` — before any tool executes (can cancel/modify)
- `post_tool_call` — after tool executes (can inspect/log result)
- `pre_turn` — before agent sends to LLM
- `post_turn` — after agent receives response
- `on_error` — on any error

Hooks are shell commands. Injects: tool name, params (JSON), result (JSON) via env vars or stdin.

### Extensions System
Extensions are Rust dynamic libraries (`.dylib`/`.so`) or WASM modules loaded at startup.

### Ratatui TUI
- **Top:** status bar (model, provider, token count)
- **Center-left:** conversation / agent output (scrollable)
- **Center-right:** tool activity / live tool output
- **Bottom:** input box (multiline, vim-ish keybindings)
- **Overlay:** file picker, help modal

### Non-interactive Mode
`ap -p "your prompt"` — run headless, print output, exit.

### Session Persistence
- Sessions saved to `~/.ap/sessions/<id>.json`
- `--session <id>` to resume

## Acceptance Criteria

- [ ] `cargo build --release` succeeds with zero warnings
- [ ] `ap -p "read Cargo.toml and summarize it"` works end-to-end with real Bedrock calls
- [ ] All 4 tools work and have unit tests
- [ ] TUI renders without crashing
- [ ] Hook system executes shell commands at correct lifecycle points
- [ ] Extension discovery loads from `~/.ap/extensions/` without crashing
- [ ] `README.md` is complete and accurate
