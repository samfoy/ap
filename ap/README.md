# ap — AI Coding Agent

A terminal AI coding agent written in Rust. Powered by AWS Bedrock (Claude), with a ratatui TUI, shell lifecycle hooks, session persistence, and a non-interactive mode for scripting.

---

## Features

- **Streaming AI responses** via AWS Bedrock (Anthropic Claude)
- **4 built-in tools**: read, write, edit, bash — fully integrated into the agent loop
- **Ratatui TUI** with conversation panel, live tool activity, and vim-style keybindings
- **Non-interactive mode** (`-p`) for scripting and use by other agents
- **Shell lifecycle hooks** — pre/post tool call, pre/post turn, on error
- **Session persistence** — save and resume conversations by ID
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

**Resume a previous session:**

```sh
ap --session <session-id>
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

## Hooks System

Hooks are shell scripts executed at lifecycle points in the agent loop. Configure hook paths in `ap.toml` or `~/.ap/config.toml`. Missing or unconfigured hooks are silently skipped.

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

`ap` automatically saves every conversation to `~/.ap/sessions/<session-id>.json`.

### Starting a new session

```sh
ap                          # generates a new UUID session ID
ap -p "summarize README"    # non-interactive also creates a session
```

The session ID is printed to stderr when a new session starts.

### Resuming a session

```sh
ap --session <session-id>
```

Previous messages are loaded and the agent continues the conversation from where you left off.

### Session file format

Sessions are stored as JSON at `~/.ap/sessions/<id>.json`:

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "created_at": "2026-03-22T14:30:00Z",
  "model": "us.anthropic.claude-sonnet-4-6",
  "messages": [ ... ]
}
```

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

See `.agents/scratchpad/implementation/ap-ai-coding-agent/design.md` for the full design document, including component diagrams, hook protocol details, and the message format used with Bedrock.

### Project layout

```
ap/
├── Cargo.toml
├── ap.toml.example          # Annotated config template
├── README.md
└── src/
    ├── main.rs              # CLI entry point (clap), TUI/headless dispatch
    ├── app.rs               # AgentLoop, conversation state, tool dispatch
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
    │   ├── mod.rs           # TuiApp, AppMode, UiEvent
    │   ├── ui.rs            # ratatui layout and rendering
    │   └── events.rs        # Keyboard event handling
    └── session/
        ├── mod.rs           # Session struct
        └── store.rs         # SessionStore, save/load JSON
```
