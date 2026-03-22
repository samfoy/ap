# Tool Discovery — Rough Idea

## Vision
`ap` is context-aware. Projects have specific workflows; tool discovery makes `ap` give Claude named, schematised tools (from `tools.toml` / `.ap/skills/*.toml`) instead of raw bash guessing.

## Technical Requirements (from scratchpad)

### Types (`src/discovery/mod.rs`)
- `DiscoveredTool`: name, description, params (IndexMap<String, ParamSpec>), command
- `DiscoveryResult`: tools: Vec<DiscoveredTool>, system_prompt_additions: Vec<String>
- `discover(root: &Path) -> DiscoveryResult` (pure, no global I/O)

### TOML Formats
- `tools.toml`: [[tool]] array with name, description, command, [tool.params] table
- `.ap/skills/*.toml`: same format with optional system_prompt string

### ShellTool
- Implements `Tool` trait
- Runs command via bash
- Injects `AP_PARAM_*` env vars for parameters

### System Prompt Threading
- `Conversation::system_prompt: Option<String>` + `with_system_prompt()`
- `Provider::stream_completion` gains `system_prompt: Option<&'a str>` param
- BedrockProvider passes it to Bedrock `system` field
- `turn()` extracts from `conv.system_prompt` and passes to provider

### 5 Steps (each independently compilable)
1. `DiscoveredTool` type + TOML serde
2. `discover()` pure function with tempfile-based tests
3. `ShellTool` implementation with execution tests
4. `Conversation`/`Provider` system prompt threading
5. Wiring discovery into `main.rs`

## Open Questions
- What should `discover()` do with malformed TOML files?
