# Technologies — tool-discovery

## Available in `ap/Cargo.toml` (main dependencies)

| Crate | Version | Relevant for |
|---|---|---|
| `serde` | 1 (with derive) | All struct deserialization |
| `serde_json` | 1 | Schema generation, JSON manipulation |
| `toml` | 0.8 | TOML file parsing |
| `tokio` | 1 (full) | Async process execution in ShellTool |
| `futures` | 0.3 | `BoxFuture`, `BoxStream` |
| `tempfile` | 3 | **Available in main deps too** (not just dev) — production I/O tests |
| `anyhow` | 1 | Error handling |
| `thiserror` | 2 | Error type definitions |

## Missing from `ap/Cargo.toml`

| Crate | Version | Why Needed |
|---|---|---|
| `indexmap` | 2 | `IndexMap<String, ParamSpec>` for insertion-order params |

**Must add to `[dependencies]` in `ap/Cargo.toml`:**
```toml
indexmap = { version = "2", features = ["serde"] }
```
The `serde` feature is required so `IndexMap` can be deserialized directly.

## `toml` crate API patterns (version 0.8)

```rust
// Parse to typed struct
let file: ToolsFile = toml::from_str(&raw_string)?;

// Parse to raw table (used in config.rs overlay approach)
let table: toml::Table = toml::from_str(&raw_string)?;
```

For `discover()`, use `toml::from_str::<ToolsFile>` directly — much simpler than the overlay approach used in `config.rs`.

## `indexmap` API patterns

```rust
use indexmap::IndexMap;

// Deserializes from TOML [tool.params] table in insertion order
let params: IndexMap<String, ParamSpec> = ...;

// Iterate in insertion order
for (key, spec) in &tool.params { ... }

// Collect required keys
let required: Vec<&str> = tool.params.iter()
    .filter(|(_, s)| s.required)
    .map(|(k, _)| k.as_str())
    .collect();
```

## `std::fs` for discovery I/O

```rust
// File existence check (no panic)
if !path.exists() { return ...; }

// Read file content
match std::fs::read_to_string(&path) {
    Ok(raw) => { /* parse */ }
    Err(e) => { warnings.push(format!("{}: {e}", path.display())); }
}

// Glob for skill files
match std::fs::read_dir(&skills_dir) {
    Ok(entries) => { /* sort + iterate */ }
    Err(_) => { /* no skills dir → skip silently */ }
}
```

## `tokio::process::Command` with env vars

```rust
use tokio::process::Command;

let mut cmd = Command::new("sh");
cmd.arg("-c").arg(&tool.command);
cmd.current_dir(&self.root);
// Inject AP_PARAM_* env vars
for (key, val) in &env_map {
    cmd.env(key, val);
}
let output = match cmd.output().await {
    Ok(o) => o,
    Err(e) => return ToolResult::err(format!("failed to spawn command: {e}")),
};
```

## `tempfile` for tests (already available)

```rust
use tempfile::TempDir;

let dir = TempDir::new().unwrap();
let tools_toml = dir.path().join("tools.toml");
std::fs::write(&tools_toml, r#"..."#).unwrap();
let result = discover(dir.path());
```

`TempDir` is already used in integration test patterns; use same approach.
