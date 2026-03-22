# ap — AI Coding Agent in Rust

Build `ap`, a terminal AI coding agent written in Rust with a ratatui TUI.

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

Tools follow a simple trait:
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;  // JSON Schema for parameters
    fn execute(&self, params: serde_json::Value) -> impl Future<Output = ToolResult> + Send;
}
```

### Hooks System
First-class lifecycle hooks, configurable via `ap.toml`:
- `pre_tool_call` — before any tool executes (can cancel/modify)
- `post_tool_call` — after tool executes (can inspect/log result)
- `pre_turn` — before agent sends to LLM
- `post_turn` — after agent receives response
- `on_error` — on any error

Hooks are shell commands. Ralph injects: tool name, params (JSON), result (JSON) via env vars or stdin.

### Extensions System
Extensions are Rust dynamic libraries (`.dylib`/`.so`) or WASM modules loaded at startup.
They can:
- Register new tools
- Register new hooks
- Add custom UI panels to the ratatui TUI
- Intercept/transform messages

Extension interface:
```rust
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn register(&self, registry: &mut Registry);
}
```

Extensions discovered from `~/.ap/extensions/` and `./.ap/extensions/`.

Config in `ap.toml` (project-level) and `~/.ap/config.toml` (global):
```toml
[provider]
backend = "bedrock"
model = "us.anthropic.claude-sonnet-4-6"
region = "us-west-2"

[tools]
# all built-ins enabled by default

[hooks]
pre_tool_call = "~/.ap/hooks/pre_tool.sh"
# etc.

[extensions]
# auto-discovered from ~/.ap/extensions/
```

### Ratatui TUI
Layout:
- **Top:** status bar (model, provider, token count)
- **Center-left:** conversation / agent output (scrollable)
- **Center-right:** tool activity / live tool output
- **Bottom:** input box (multiline, vim-ish keybindings)
- **Overlay:** file picker, help modal

Key bindings:
- `i` / `Enter` — focus input
- `Esc` — back to normal mode
- `Ctrl+C` — quit
- `Ctrl+L` — clear screen
- `/help` — show keybindings

### Non-interactive Mode
`ap -p "your prompt"` — run headless, print output, exit. Good for scripting and being driven by Ralph.

### Session Persistence
- Sessions saved to `~/.ap/sessions/<id>.json`
- `--session <id>` to resume

## Project Structure

```
ap/
├── Cargo.toml
├── ap.toml.example
├── src/
│   ├── main.rs
│   ├── app.rs           # App state
│   ├── config.rs        # Config loading (ap.toml)
│   ├── provider/
│   │   ├── mod.rs
│   │   └── bedrock.rs   # AWS Bedrock provider
│   ├── tools/
│   │   ├── mod.rs
│   │   ├── read.rs
│   │   ├── write.rs
│   │   ├── edit.rs
│   │   └── bash.rs
│   ├── hooks/
│   │   ├── mod.rs
│   │   └── runner.rs
│   ├── extensions/
│   │   ├── mod.rs
│   │   └── loader.rs
│   ├── tui/
│   │   ├── mod.rs
│   │   ├── ui.rs
│   │   └── events.rs
│   └── session/
│       ├── mod.rs
│       └── store.rs
└── README.md
```

## Implementation Plan

Implement in order — each step should compile and be testable:

1. **Cargo.toml + project scaffold** — workspace, all deps, basic `main.rs` that prints version
2. **Config system** — `ap.toml` loading with serde, defaults, merge global + project
3. **Tool trait + 4 built-in tools** — read, write, edit, bash with unit tests
4. **Bedrock provider** — streaming API calls, message formatting for Claude, tool call parsing
5. **Hooks system** — shell command runner, env var injection, pre/post hooks
6. **Extensions system** — discovery, loading interface (trait object, no actual dylib loading needed in v1 — stub it)
7. **Agent loop** — conversation state, tool dispatch, streaming output, error handling
8. **Session persistence** — save/load JSON sessions
9. **Ratatui TUI** — layout, input box, scrollable output, tool activity panel
10. **Non-interactive mode** — `-p` flag, headless operation
11. **README.md** — usage, config reference, extension/hook docs
12. **Final polish** — `cargo clippy`, `cargo test`, fix all warnings

## Acceptance Criteria

- [ ] `cargo build --release` succeeds with zero warnings
- [ ] `ap -p "read Cargo.toml and summarize it"` works end-to-end with real Bedrock calls
- [ ] All 4 tools work and have unit tests
- [ ] TUI renders without crashing
- [ ] Hook system executes shell commands at correct lifecycle points
- [ ] Extension discovery loads from `~/.ap/extensions/` without crashing
- [ ] `README.md` is complete and accurate

## Notes

- Commit frequently with conventional commits (feat/fix/chore/refactor)
- Don't over-engineer v1 — clean interfaces, solid foundation
- The extension system in v1 can be interface-only (trait defined, no actual dylib loading) — what matters is the API is right
- Hooks in v1: shell commands only, no scripting API needed yet
- Output LOOP_COMPLETE when all acceptance criteria are met and the project builds clean
