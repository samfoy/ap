# Tool Discovery — Consolidated Requirements

*Synthesized 2026-03-22 from rough-idea.md + idea-honing.md*

---

## Functional Requirements

### FR-1 — Discovery Types (`src/discovery/mod.rs`)
The crate must expose a `DiscoveryResult` struct with:
- `tools: Vec<DiscoveredTool>` — successfully parsed tools
- `system_prompt_additions: Vec<String>` — extra system prompt snippets from skill files
- `warnings: Vec<String>` — human-readable messages about malformed/skipped files

Each `DiscoveredTool` must have:
- `name: String` — unique tool identifier (used as the tool name Claude calls)
- `description: String` — human/LLM-readable purpose description
- `params: IndexMap<String, ParamSpec>` — ordered parameter map
- `command: String` — shell command template (may reference env vars)

Each `ParamSpec` must have:
- `description: String` — what the parameter means
- `required: bool` — whether Claude must supply it (default `true` when omitted from TOML)

### FR-2 — Discovery Function
`pub fn discover(root: &Path) -> DiscoveryResult` must:
- Be a pure function (no global I/O, no process-level side effects)
- Scan `{root}/tools.toml` for `[[tool]]` entries
- Scan `{root}/.ap/skills/*.toml` for `[[tool]]` entries and optional `system_prompt` strings
- Return all successfully parsed tools across all files
- On malformed TOML or missing required fields: add a warning string, skip that file, continue
- Not panic

### FR-3 — TOML File Formats
`tools.toml` format:
```toml
[[tool]]
name = "run-tests"
description = "Run the test suite"
command = "cargo test $AP_PARAM_FILTER"

[tool.params.filter]
description = "Test filter glob (optional)"
required = false
```

`.ap/skills/*.toml` format (same as above, plus optional top-level `system_prompt`):
```toml
system_prompt = "You have access to Rust project tools."

[[tool]]
name = "build"
description = "Build the project"
command = "cargo build $AP_PARAM_PROFILE"

[tool.params.profile]
description = "Build profile: debug or release"
# required defaults to true
```

### FR-4 — ShellTool
`ShellTool` must implement the existing `Tool` trait:
- `name()` returns the `DiscoveredTool.name`
- `description()` returns the `DiscoveredTool.description`
- `schema()` emits a JSON Schema object matching the Bedrock/Anthropic tool format:
  - Properties from `params`, each with `type: "string"` and `description`
  - `required` array contains only params where `required: true`
- `execute(params)`:
  - Set `AP_PARAM_{KEY_UPPERCASE}` env vars for each supplied param
  - For each `required: true` param absent from input: return `ToolResult::err("missing required parameter: {key}")`
  - Run the command via `sh -c` (like `BashTool`)
  - Capture stdout + stderr + exit code in `ToolResult::ok(...)` (non-zero exit is not a tool error, consistent with `BashTool`)

### FR-5 — System Prompt Threading
`Conversation` must gain:
- `system_prompt: Option<String>` field (serde default `None`)
- `with_system_prompt(self, prompt: impl Into<String>) -> Self` builder

`Provider::stream_completion` signature must change to:
```rust
fn stream_completion<'a>(
    &'a self,
    messages: &'a [Message],
    tools: &'a [serde_json::Value],
    system_prompt: Option<&'a str>,
) -> BoxStream<'a, Result<StreamEvent, ProviderError>>;
```

`BedrockProvider` must pass `system_prompt` to the `"system"` field of the Bedrock API request body (when `Some`).

`turn()` must extract `conv.system_prompt.as_deref()` and forward it to `provider.stream_completion`.

### FR-6 — main.rs Wiring
At startup, `main.rs` must:
1. Determine the project root (current working directory)
2. Call `discover(&project_root)` → `DiscoveryResult`
3. Print each `discovery.warnings` entry via `eprintln!("ap: {w}")`
4. Construct `ShellTool` for each `discovery.tools` entry and register with `ToolRegistry`
5. Join `discovery.system_prompt_additions` with `"\n\n"` and call `conv.with_system_prompt(combined)` if non-empty

---

## Non-Functional Requirements

### NFR-1 — Purity
`discover()` must be testable with `tempfile::TempDir` — no global paths.

### NFR-2 — Safety Lints
All new code must pass the existing lint gates:
- `#![deny(unsafe_code)]`
- `#![deny(clippy::unwrap_used)]`
- `#![deny(clippy::expect_used)]`
- `#![warn(clippy::pedantic)]`

### NFR-3 — Independently Compilable Steps
Each of the 5 implementation steps must leave the codebase in a compilable state.

### NFR-4 — Dependency
`indexmap` crate must be added to `Cargo.toml` (for insertion-order param maps in TOML).

---

## Out of Scope
- `ParamType` enum (type-rich schema beyond `"string"`) — deferred
- Tool discovery from environment variables or registry files
- Remote tool discovery
- Tool versioning
