# ap — AI Coding Agent

A terminal AI coding agent written in Rust. Powered by AWS Bedrock (Claude), with a ratatui TUI, a composable middleware chain, shell lifecycle hooks, session persistence, and a non-interactive mode for scripting.

---

## Features

- **Streaming AI responses** via AWS Bedrock (Anthropic Claude)
- **4 built-in tools**: read, write, edit, bash — fully integrated into the turn pipeline
- **Ratatui TUI** with conversation panel, live tool activity, and vim-style keybindings
- **Non-interactive mode** (`-p`) for scripting and use by other agents
- **Composable middleware chain** — intercept, block, or transform tool calls with Rust closures
- **Shell lifecycle hooks** — pre/post tool call, pre/post turn, on error (wrapped as middleware)
- **Session persistence** — opt-in save and resume of conversations via `--session <id>`
- **Layered config** — global (`~/.ap/config.toml`) + project (`ap.toml`) with field-level merge

---

## Installation

### Prerequisites

- **Rust stable toolchain** — install via [rustup](https://rustup.rs)
- **AWS credentials** configured (see [AWS Setup](#aws-setup))

### Build from source

```sh
git clone <repo>
cd ap
cargo build --release
# Binary at: ./target/release/ap
```

To install into your `PATH`:

```sh
cargo install --path .
```

---

## Quick Start

**Non-interactive (scripting/one-shot):**

```sh
ap -p "read Cargo.toml and summarize it"
```

**Interactive TUI:**

```sh
ap
```

**Start or resume a named session:**

```sh
ap --session my-project
```

---

## AWS Setup

`ap` uses **AWS Bedrock** to call Claude. Standard AWS SDK credential resolution applies — no special configuration is required if you already have AWS credentials set up.

### Credential sources (checked in order)

1. Environment variables: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`
2. AWS credentials file: `~/.aws/credentials`
3. IAM instance/task role (EC2, ECS, Lambda)

### Required IAM permissions

Your IAM principal needs `bedrock:InvokeModelWithResponseStream` on the model ARN:

```
arn:aws:bedrock:us-west-2::foundation-model/us.anthropic.claude-sonnet-4-6
```

### Region

Default region is `us-west-2`. Override via `ap.toml` or the `AWS_DEFAULT_REGION` environment variable.

### Model

Default model: `us.anthropic.claude-sonnet-4-6`. Override in `ap.toml`:

```toml
[provider]
model = "us.anthropic.claude-3-5-haiku-20241022-v1:0"
region = "us-east-1"
```

---

## Configuration

Config files are **optional** — `ap` starts with sensible defaults if neither file exists.

| File | Scope | Priority |
|------|-------|----------|
| `~/.ap/config.toml` | Global (all projects) | Lower |
| `./ap.toml` | Project (current directory) | Higher |

When both files are present, project config wins — but only for keys that are explicitly set. A project config that only overrides `model` will still inherit `region` from the global config.

### Full reference (`ap.toml`)

```toml
[provider]
# AI backend. Only "bedrock" is supported in v1.
backend = "bedrock"

# Bedrock model identifier.
model = "us.anthropic.claude-sonnet-4-6"

# AWS region for Bedrock API calls.
region = "us-west-2"

[tools]
# Built-in tools enabled for the agent. All four are enabled by default.
enabled = ["read", "write", "edit", "bash"]

[hooks]
# Shell script run before any tool executes.
# Non-zero exit cancels the tool call.
# pre_tool_call = "~/.ap/hooks/pre_tool.sh"

# Shell script run after any tool executes.
# stdout from the script replaces the tool result content.
# post_tool_call = "~/.ap/hooks/post_tool.sh"

# Shell script run before the agent sends a turn to the LLM (read-only).
# pre_turn = "~/.ap/hooks/pre_turn.sh"

# Shell script run after the agent receives a response (read-only).
# post_turn = "~/.ap/hooks/post_turn.sh"

# Shell script run on any agent error (read-only).
# on_error = "~/.ap/hooks/on_error.sh"
```

---

## Built-in Tools

All four tools are enabled by default and available to the agent in every turn.

| Tool | Description | Required Parameters | Optional Parameters |
|------|-------------|--------------------|--------------------|
| `read` | Read the contents of a file | `path` (string) | — |
| `write` | Write content to a file, creating parent directories as needed | `path` (string), `content` (string) | — |
| `edit` | Replace a unique occurrence of `old_text` with `new_text` in a file. Errors if `old_text` is not found or matches more than once | `path` (string), `old_text` (string), `new_text` (string) | — |
| `bash` | Run a shell command via `sh -c` and return stdout, stderr, and exit code | `command` (string) | — |

### Tool notes

- **read** — Returns raw file contents as a string. Fails gracefully with an error message if the file does not exist.
- **write** — Creates the file and any missing parent directories. Overwrites existing files.
- **edit** — Requires `old_text` to appear **exactly once** in the file. Use `read` first to verify uniqueness when in doubt.
- **bash** — No timeout in v1. Commands run to completion. Both stdout and stderr are captured and returned to the agent.

---

## Architecture

`ap` is built as a functional pipeline. Each agent turn is a pure data transformation with no hidden state:

```rust
pub async fn turn(
    conv: Conversation,
    provider: &dyn Provider,
    tools: &ToolRegistry,
    middleware: &Middleware,
) -> Result<(Conversation, Vec<TurnEvent>)>
```

`turn()` takes an immutable `Conversation` (with the user message already appended) and returns the updated `Conversation` plus a `Vec<TurnEvent>` — no side effects. The caller routes events to the TUI channel or stdout.

Internally the pipeline is a sequence of pure steps:
1. `apply_pre_turn(conv, middleware)` — run pre-turn middleware
2. `stream_completion(conv, provider)` — stream LLM response, collect events and tool calls
3. For each tool call: run pre-tool middleware chain → execute tool → run post-tool middleware chain
4. Append assistant message and tool results to conversation
5. Loop until no more tool calls are pending
6. `apply_post_turn(conv, middleware)` — run post-turn middleware

### Conversation

`Conversation` is an immutable value type — each turn consumes the old value and returns a new one:

```rust
pub struct Conversation {
    pub id: String,
    pub model: String,
    pub messages: Vec<Message>,
    pub config: AppConfig,
}
```

Build a new turn by calling `conv.with_user_message(input)`, which returns a new `Conversation` with the user message appended.

### TurnEvent

Both the TUI and headless mode consume the same `TurnEvent` stream:

```rust
pub enum TurnEvent {
    TextChunk(String),                        // streamed text fragment
    ToolStart { name: String, params: Value },
    ToolComplete { name: String, result: String },
    TurnEnd,
    Error(String),
}
```

---

## Middleware API

The `Middleware` chain is the primary extension point. Add Rust closures to intercept, block, or transform tool calls at any point in the pipeline.

```rust
pub struct Middleware {
    pub pre_turn:  Vec<TurnMiddlewareFn>,   // Fn(&Conversation) -> Option<Conversation>
    pub post_turn: Vec<TurnMiddlewareFn>,
    pub pre_tool:  Vec<ToolMiddlewareFn>,   // Fn(ToolCall) -> ToolMiddlewareResult
    pub post_tool: Vec<ToolMiddlewareFn>,
}
```

`ToolMiddlewareResult` controls what happens to each tool call:

```rust
pub enum ToolMiddlewareResult {
    Allow(ToolCall),      // pass through (possibly modified)
    Block(String),        // cancel — the string is returned to Claude as the result
    Transform(ToolResult),// skip execution, return this result directly
}
```

### Builder pattern

Chain middleware in `main.rs` using the consuming builder API:

```rust
let middleware = Middleware::new()
    .pre_tool(|call| {
        // Log every tool call
        eprintln!("[tool] {}: {}", call.name, call.params);
        ToolMiddlewareResult::Allow(call)
    })
    .pre_tool(|call| {
        // Block dangerous bash commands
        if call.name == "bash" {
            let cmd = call.params["command"].as_str().unwrap_or("");
            if cmd.contains("rm -rf") {
                return ToolMiddlewareResult::Block("rm -rf is not allowed".into());
            }
        }
        ToolMiddlewareResult::Allow(call)
    });
```

### Pre-turn middleware

Turn middleware receives a `&Conversation` and returns `Option<Conversation>`. Return `None` to leave the conversation unchanged, or `Some(new_conv)` to replace it:

```rust
let middleware = Middleware::new()
    .pre_turn(|conv| {
        // Observe the conversation before each LLM call
        eprintln!("[turn] {} messages so far", conv.messages.len());
        None // no modification
    });
```

### Multiple middleware functions

All middleware functions in a chain run in order. For pre-tool middleware, the first `Block` or `Transform` result short-circuits the chain — subsequent middleware is not called.

---

## Hooks System

Shell hooks are the configuration-based extension point. Under the hood, configured hook scripts are **automatically wrapped as `Middleware` closures** at startup via `shell_hook_bridge()`. This means shell hooks and Rust middleware coexist in the same chain — hooks are not a separate system.

Configure hook paths in `ap.toml` or `~/.ap/config.toml`. Missing or unconfigured hooks are silently skipped.

### Lifecycle events

| Hook | Trigger | Can cancel? | stdout effect |
|------|---------|-------------|---------------|
| `pre_tool_call` | Before any tool executes | ✅ Yes — non-zero exit cancels the tool | Ignored |
| `post_tool_call` | After any tool executes | ❌ No | Non-empty stdout **replaces** the tool result content |
| `pre_turn` | Before the agent sends to the LLM | ❌ No (advisory warning only) | Ignored |
| `post_turn` | After the agent receives an LLM response | ❌ No (advisory warning only) | Ignored |
| `on_error` | On any agent error | ❌ No (advisory warning only) | Ignored |

### Environment variables

#### `pre_tool_call`

| Variable | Value |
|----------|-------|
| `AP_TOOL_NAME` | Name of the tool being called (e.g. `bash`) |
| `AP_TOOL_PARAMS` | Tool parameters as a JSON string |

#### `post_tool_call`

| Variable | Value |
|----------|-------|
| `AP_TOOL_NAME` | Name of the tool that was called |
| `AP_TOOL_PARAMS` | Tool parameters as a JSON string |
| `AP_TOOL_RESULT` | Tool result as a JSON string |
| `AP_TOOL_IS_ERROR` | `"true"` if the tool returned an error, `"false"` otherwise |

#### `pre_turn`, `post_turn`, `on_error`

| Variable | Value |
|----------|-------|
| `AP_MESSAGES_FILE` | Path to a temp file containing the current messages array as JSON |

### Hook examples

**Approval gate — require confirmation before any bash command:**

```sh
#!/bin/sh
# ~/.ap/hooks/pre_tool.sh
if [ "$AP_TOOL_NAME" = "bash" ]; then
  echo "Command: $(echo "$AP_TOOL_PARAMS" | jq -r .command)" >&2
  printf "Allow? [y/N] " >&2
  read -r answer </dev/tty
  [ "$answer" = "y" ] || exit 1
fi
```

**Audit log — record every tool call:**

```sh
#!/bin/sh
# ~/.ap/hooks/post_tool.sh
echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) tool=$AP_TOOL_NAME is_error=$AP_TOOL_IS_ERROR" \
  >> ~/.ap/audit.log
```

**Restrict bash to a safe list of commands:**

```sh
#!/bin/sh
# ~/.ap/hooks/pre_tool.sh
if [ "$AP_TOOL_NAME" = "bash" ]; then
  CMD=$(echo "$AP_TOOL_PARAMS" | jq -r .command)
  case "$CMD" in
    ls*|cat*|echo*) exit 0 ;;
    *) echo "command not in allowlist" >&2; exit 1 ;;
  esac
fi
```

---

## Session Management

Session persistence is **opt-in** — `ap` only saves conversation history when `--session <id>` is explicitly provided. Running `ap` without `--session` is ephemeral: nothing is written to disk.

### Starting a named session

```sh
ap --session my-project
ap -p "summarize README" --session my-project
```

Provide any string as the session ID. If `~/.ap/sessions/<id>.json` does not exist, a new session is created and saved there. If it already exists, the conversation history is loaded and the agent continues from where you left off.

### Resuming a session

```sh
ap --session my-project
```

Previous messages are loaded automatically. The agent picks up the conversation from the last saved state.

### Ephemeral (no persistence)

```sh
ap                          # no --session flag → nothing is saved
ap -p "one-off question"    # non-interactive without --session is also ephemeral
```

### Session file format

Sessions are stored as serialized `Conversation` values at `~/.ap/sessions/<id>.json`:

```json
{
  "id": "my-project",
  "model": "us.anthropic.claude-sonnet-4-6",
  "messages": [ ... ],
  "config": { ... }
}
```

The `Conversation` type is fully serializable — `save_conversation` / `load_conversation` in `SessionStore` handle the JSON encoding. The `config` field uses `#[serde(default)]` so sessions saved without a config field load cleanly with defaults.

---

## Non-Interactive Mode

Run `ap` with `-p` / `--prompt` to operate headlessly — no TUI, output to stdout/stderr, exits when done.

```sh
ap -p "read src/main.rs and explain what it does"
```

### Output

- **stdout** — streamed AI response text (flushed incrementally)
- **stderr** — tool start/complete notifications

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Turn completed successfully |
| `1` | Provider error or agent failure |

### Scripting example

```sh
# Summarize changed files before a commit
SUMMARY=$(ap -p "summarize the changes in $(git diff --name-only HEAD)")
git commit -m "$SUMMARY"
```

```sh
# Use in a CI pipeline
ap -p "review src/ for obvious bugs and output a report" > review.txt
```

---

## TUI Keybindings

### Normal mode

| Key | Action |
|-----|--------|
| `i` | Enter insert mode (focus input) |
| `Enter` | Enter insert mode (focus input) |
| `j` / `Page Down` | Scroll conversation down |
| `k` / `Page Up` | Scroll conversation up |
| `Ctrl+C` | Quit |
| `Esc` | Dismiss help overlay |

### Insert mode

| Key | Action |
|-----|--------|
| `Enter` | Submit message to agent |
| `Esc` | Return to normal mode |
| `Backspace` | Delete last character |
| `Ctrl+C` | Quit |
| Any printable key | Append to input buffer |

### Special commands (insert mode)

| Input | Action |
|-------|--------|
| `/help` + Enter | Show keybindings overlay |

---

## Contributing

### Running tests

```sh
cd ap
cargo test
```

### Build

```sh
cargo build --release
```

### Linting

```sh
cargo clippy -- -D warnings
```

### Architecture

`main.rs` is the composition root — it wires tools, middleware, provider, and conversation together. There is no extension system: to add new behavior, edit the source. The pipeline is intentionally transparent.

### Project layout

```
ap/
├── Cargo.toml
├── ap.toml.example          # Annotated config template
├── README.md
└── src/
    ├── main.rs              # CLI entry point — wires tools, middleware, provider; TUI/headless dispatch
    ├── types.rs             # Core types: Conversation, TurnEvent, ToolCall, Middleware
    ├── turn.rs              # Pure turn() pipeline function
    ├── middleware.rs        # Middleware builder API + shell_hook_bridge()
    ├── config.rs            # AppConfig, layered TOML loading
    ├── provider/
    │   ├── mod.rs           # Provider trait, StreamEvent, Message types
    │   └── bedrock.rs       # AWS Bedrock streaming provider
    ├── tools/
    │   ├── mod.rs           # Tool trait, ToolRegistry, ToolResult
    │   ├── read.rs
    │   ├── write.rs
    │   ├── edit.rs
    │   └── bash.rs
    ├── hooks/
    │   ├── mod.rs           # HookOutcome enum
    │   └── runner.rs        # HookRunner, shell exec, env injection
    ├── tui/
    │   ├── mod.rs           # TuiApp — wired to TurnEvent, no AgentLoop dependency
    │   ├── ui.rs            # ratatui layout and rendering
    │   └── events.rs        # Keyboard event handling
    └── session/
        ├── mod.rs           # Session struct
        └── store.rs         # SessionStore: save/load Conversation as JSON
```
