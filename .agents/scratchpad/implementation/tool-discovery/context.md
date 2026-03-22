# Context — tool-discovery

## Summary of Research Findings

The codebase is clean, well-structured Rust following a functional-first style. All integration points are understood.

---

## Integration Points and Dependencies

### Step 1: `src/discovery/mod.rs` (new) + `Cargo.toml`

**Cargo.toml change required:**
```toml
indexmap = { version = "2", features = ["serde"] }
```
Without `features = ["serde"]`, `IndexMap` won't deserialize from TOML.

**`lib.rs` change required:**
```rust
pub mod discovery;
```

No other existing code needs changing in Step 1.

### Step 2: `src/discovery/mod.rs` (add `discover()`)

Pure function using `std::fs`. Test with `tempfile::TempDir` (already in main deps, not just dev).

**Skills dir glob pattern:**
```rust
let skills_dir = root.join(".ap").join("skills");
// read_dir + collect + sort + iterate
```

**Deduplication:** `HashSet<String>` maintained across all files. Order: `tools.toml` first, then `.ap/skills/*.toml` sorted alphabetically.

### Step 3: `src/tools/shell.rs` (new) + `src/tools/mod.rs`

**`mod.rs` changes needed:**
1. Add `pub mod shell;`
2. Add `pub use shell::ShellTool;`

**ShellTool struct:**
```rust
pub struct ShellTool {
    tool: DiscoveredTool,
    root: PathBuf,
}

impl ShellTool {
    pub fn new(tool: DiscoveredTool, root: PathBuf) -> Self { Self { tool, root } }
}
```

**Env var injection pattern:**
```rust
let upper_key = format!("AP_PARAM_{}", key.to_uppercase().replace('-', "_"));
cmd.env(&upper_key, val_str);
```

**Missing required param check:** iterate `tool.params` entries where `spec.required == true`, check if present in `params` JSON object.

**Import from discovery:** `use crate::discovery::DiscoveredTool;`

### Step 4: `src/types.rs`, `src/provider/mod.rs`, `src/provider/bedrock.rs`, `src/turn.rs`

**ALL call sites of `stream_completion` must be updated. Affected locations:**

1. `ap/src/provider/bedrock.rs:139` — `impl Provider for BedrockProvider` (the real impl)
2. `ap/src/provider/bedrock.rs:153` — calls `Self::build_request_body(messages, tools)` (add `system_prompt` arg)
3. `ap/src/turn.rs:103` — `provider.stream_completion(&messages_snapshot, &tool_schemas)` (add `conv.system_prompt.as_deref()`)
4. `ap/src/turn.rs:220` — `MockProvider::stream_completion` in unit tests (add `_system_prompt` param)
5. `ap/tests/noninteractive.rs:37` — `MockProvider::stream_completion` in integration tests (add `_system_prompt` param)
6. `ap/src/provider/mod.rs` — `Provider` trait definition itself

**`Conversation` change:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub config: AppConfig,
    #[serde(default)]
    pub system_prompt: Option<String>,  // NEW — serde(default) preserves old session files
}

impl Conversation {
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }
}
```

**`Provider` trait change:**
```rust
pub trait Provider: Send + Sync {
    fn stream_completion<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [serde_json::Value],
        system_prompt: Option<&'a str>,  // NEW
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>>;
}
```

**`BedrockProvider` change:**
- `build_request_body` gains `system_prompt: Option<&str>` param
- Adds `if let Some(sp) = system_prompt { body["system"] = json!(sp); }`
- `stream_completion` passes `system_prompt` to `build_request_body`

**`turn_loop` change:**
```rust
let system_prompt = conv.system_prompt.as_deref();
let mut stream = provider.stream_completion(&messages_snapshot, &tool_schemas, system_prompt);
```

### Step 5: `src/main.rs`

**Two functions to update:** `run_headless` and `run_tui`

Both need this block added BEFORE `ToolRegistry::with_defaults()`:
```rust
let project_root = std::env::current_dir()
    .unwrap_or_else(|_| std::path::PathBuf::from("."));
let discovery = ap::discovery::discover(&project_root);
for w in &discovery.warnings {
    eprintln!("ap: {w}");
}
```

Then after `ToolRegistry::with_defaults()`:
```rust
let mut tools = ToolRegistry::with_defaults();
for dt in discovery.tools {
    tools.register(Box::new(ap::tools::ShellTool::new(dt, project_root.clone())));
}
```

And when building `conv`:
```rust
let system_prompt: Option<String> = if discovery.system_prompt_additions.is_empty() {
    None
} else {
    Some(discovery.system_prompt_additions.join("\n\n"))
};
let conv = Conversation::new(...);
let conv = match system_prompt {
    Some(sp) => conv.with_system_prompt(sp),
    None => conv,
};
```

**NOTE on `run_tui`:** The `tools` variable is wrapped in `Arc::new(...)` — must register ShellTools before wrapping.

---

## Constraints Discovered

### 1. `indexmap` must include `serde` feature
Without `features = ["serde"]`, deserialization of `IndexMap<String, ParamSpec>` from TOML won't compile.

### 2. Lint rules are strict — no `.unwrap()` outside tests
All production code (discovery, ShellTool) must use pattern matching or `?` operator. Test modules need `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`.

### 3. `Provider` trait has 3 `impl` blocks to update
Turn tests inline `MockProvider`, integration test has its own `MockProvider`, and real `BedrockProvider` — all three must be updated together in Step 4 or compilation fails.

### 4. `current_dir()` in main.rs
Design says `unwrap_or_else(|_| PathBuf::from("."))` — but `unwrap_used` lint is active. Must use `.unwrap_or_else(|_| ...)` which is fine (not `.unwrap()`).

### 5. `tempfile` in main (not just dev) deps
`tempfile = "3"` appears in both `[dependencies]` and `[dev-dependencies]`. This is redundant but harmless. Discovery tests can use `TempDir` normally.

### 6. `DiscoveryResult` must be in `ap::discovery` (public module)
`main.rs` imports from `ap::discovery::discover`. `lib.rs` must export the module.

### 7. Session file backward compatibility
Existing `.ap/sessions/*.json` files don't have `system_prompt` field. `#[serde(default)]` on `Conversation.system_prompt` ensures they deserialize without error.

### 8. `tools` in `run_tui` is wrapped in `Arc`
```rust
// BEFORE ShellTool registration:
let mut tools = ToolRegistry::with_defaults();
for dt in discovery.tools {
    tools.register(Box::new(ShellTool::new(dt, project_root.clone())));
}
// THEN wrap:
let tools = Arc::new(tools);
```

### 9. `read_dir` sort order for determinism
`std::fs::read_dir` order is undefined (OS-dependent). Must collect entries into a `Vec`, sort by filename, then iterate.

### 10. `#[serde(rename = "tool")]` in TOML
TOML uses `[[tool]]` array syntax. The `#[serde(rename = "tool")]` attribute maps the Rust field name `tools` to the TOML key `tool`.

---

## File Touch Map (by Step)

| Step | Files |
|---|---|
| 1 | `ap/Cargo.toml` (+indexmap), `ap/src/lib.rs` (+discovery), `ap/src/discovery/mod.rs` (new) |
| 2 | `ap/src/discovery/mod.rs` (add discover() + tests) |
| 3 | `ap/src/tools/shell.rs` (new), `ap/src/tools/mod.rs` (+shell) |
| 4 | `ap/src/types.rs`, `ap/src/provider/mod.rs`, `ap/src/provider/bedrock.rs`, `ap/src/turn.rs`, `ap/tests/noninteractive.rs` |
| 5 | `ap/src/main.rs` |
