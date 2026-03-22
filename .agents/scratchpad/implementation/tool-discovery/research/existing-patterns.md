# Existing Patterns — tool-discovery

## Tool Implementation Pattern (`ap/src/tools/bash.rs`)

All tools follow this exact structure:

```rust
// ap/src/tools/bash.rs:1-73
use futures::future::BoxFuture;
use serde_json::Value;
use tokio::process::Command;
use crate::tools::{Tool, ToolResult};

pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &str { "bash" }
    fn description(&self) -> &str { "..." }
    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "bash",
            "description": "...",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "..." }
                },
                "required": ["command"]
            }
        })
    }
    fn execute(&self, params: Value) -> BoxFuture<'_, ToolResult> {
        Box::pin(async move {
            let command = match params.get("command").and_then(|v| v.as_str()) {
                Some(c) => c.to_owned(),
                None => return ToolResult::err("missing required parameter: command"),
            };
            let output = match Command::new("sh").arg("-c").arg(&command).output().await {
                Ok(o) => o,
                Err(e) => return ToolResult::err(format!("failed to spawn command: {}", e)),
            };
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);
            ToolResult::ok(format!("{}\n{}\nexit: {}", stdout, stderr, exit_code))
        })
    }
}
```

**ShellTool MUST follow this exact pattern:**
- `BoxFuture<'_, ToolResult>` return type
- `Box::pin(async move { ... })`
- `Command::new("sh").arg("-c")` for execution
- `ToolResult::ok(format!("{}\n{}\nexit: {}", ...))` for success
- `ToolResult::err("missing required parameter: {key}")` for missing required params
- `ToolResult::err(format!("failed to spawn command: {}", e))` for spawn failures
- Non-zero exit code is NOT a tool error — always `ToolResult::ok` with exit code captured

For ShellTool, `Command::env(key, val)` or `Command::envs(iter)` must be used to inject `AP_PARAM_*` env vars.

## Schema Pattern (JSON shape sent to Claude)

```json
{
  "name": "{name}",
  "description": "{description}",
  "input_schema": {
    "type": "object",
    "properties": {
      "{key}": { "type": "string", "description": "{desc}" }
    },
    "required": ["{required_keys...}"]
  }
}
```

Required keys omitted entirely when empty (but an empty `required` array is harmless).

## Config Loading Pattern (`ap/src/config.rs`)

- TOML parsing uses `toml::from_str::<Type>(&raw)` (not `toml::Value`)
- `#[serde(default)]` on top-level structs for missing fields
- `AppConfig::load()` is infallible at type level but returns `Result` — callers use `.unwrap_or_default()`
- `tools.toml` format is analogous: use `toml::from_str::<ToolsFile>(&raw)` pattern

## Conversation New Pattern (`ap/src/types.rs:28-40`)

```rust
Conversation::new(id, model, config)  // creates empty
    .with_user_message("text")         // builder, consumes self
```

Builder pattern with `mut self` → returns `Self`. `with_system_prompt` should follow same pattern.

## ToolRegistry Registration (`ap/src/tools/mod.rs:60-75`)

```rust
pub fn with_defaults() -> Self {
    let mut registry = Self::new();
    registry.register(Box::new(ReadTool));
    // ...
    registry
}
pub fn register(&mut self, tool: Box<dyn Tool>) {
    self.tools.push(tool);
}
```

`ShellTool::new(discovered, root)` → `Box::new(...)` → `registry.register(...)` is the wiring pattern.

## Error Handling in main.rs

```rust
let config = AppConfig::load().unwrap_or_default();
// ...
let store = SessionStore::new().unwrap_or_else(|e| { eprintln!(...); fallback });
```

Discovery warnings follow same pattern: `for w in &discovery.warnings { eprintln!("ap: {w}"); }`.

## Linting Rules (`ap/Cargo.toml:lints`)

```toml
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
needless_pass_by_ref_mut = "deny"
```

**All new code must NOT use `.unwrap()`, `.expect()`, or `panic!()`** outside `#[cfg(test)]` blocks.
Test modules use `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`.

## Module Export Pattern (`ap/src/lib.rs`)

```rust
pub mod config;
pub mod hooks;
// ... one line per module
```

`pub mod discovery;` must be added to `lib.rs`.
`pub mod shell;` must be added to `tools/mod.rs` and re-exported.

## Async Command Pattern (`tokio::process::Command`)

```rust
use tokio::process::Command;
// ...
let output = match Command::new("sh").arg("-c").arg(&command).output().await {
    Ok(o) => o,
    Err(e) => return ToolResult::err(format!("failed to spawn command: {}", e)),
};
```

For env vars: `Command::new("sh").arg("-c").arg(&cmd).envs(env_map).output().await`

## TOML Parsing for Config

Config uses `toml::Table` (raw) overlay approach, but for discovery, simpler `toml::from_str::<SkillFile>(&raw)` is correct — `toml` crate already in main deps.
