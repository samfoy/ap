---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Config System

## Description
Implement `src/config.rs` with typed configuration structs that load from `ap.toml` (project-level) and `~/.ap/config.toml` (global), merge them (project overrides global), and fill in defaults. Return a typed error with file path context on invalid TOML.

## Background
Config is loaded once at startup and injected into every subsystem. Getting the merge semantics right (project overrides global, defaults fill missing) is important for the user experience. Both config files are optional — the app must run with zero config files present.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/ap-ai-coding-agent/design.md

**Additional References:**
- .agents/scratchpad/implementation/ap-ai-coding-agent/plan.md (overall strategy)

**Note:** You MUST read the design document before beginning implementation. Section 4.2 covers config structure.

## Technical Requirements
1. `AppConfig` struct with nested sub-configs, all fields `#[serde(default)]`:
   - `ProviderConfig { backend: String, model: String, region: String }` — default: bedrock, `us.anthropic.claude-sonnet-4-6`, `us-west-2`
   - `HooksConfig { pre_tool_call, post_tool_call, pre_turn, post_turn, on_error: Option<String> }` — all default None
   - `ToolsConfig { enabled: Vec<String> }` — default: all 4 built-ins
   - `ExtensionsConfig { auto_discover: bool }` — default true
2. `AppConfig::load() -> anyhow::Result<AppConfig>`:
   - Try to read `~/.ap/config.toml` (global) — if present, parse it; if absent, use defaults
   - Try to read `./ap.toml` (project) — if present, parse it; if absent, use defaults
   - Merge: project values override global values; any None/missing field falls back to global or default
   - On parse error: return `Err` with file path in context (e.g., `"failed to parse ap.toml at ./ap.toml"`)
3. Implement `Default` for all config structs with the specified defaults
4. All structs derive `Debug`, `Clone`, `Serialize`, `Deserialize`
5. Use `dirs::home_dir()` for `~/.ap/` path resolution

## Dependencies
- Task 01 (project scaffold) must be complete — `Cargo.toml` must declare `toml`, `serde`, `anyhow`, `dirs`

## Implementation Approach
1. Write all 5 unit tests first (RED phase):
   - `test_defaults_when_no_file` — no config files → defaults returned
   - `test_load_project_config` — write temp `ap.toml`, verify it loads
   - `test_global_config_merged` — write temp `~/.ap/config.toml` equivalent, verify merge
   - `test_project_overrides_global` — both files present, project value wins
   - `test_invalid_toml_returns_error` — malformed TOML → error with path context
2. Implement `AppConfig`, sub-structs, `Default` impls (GREEN phase)
3. Implement `AppConfig::load()` with merge logic (GREEN phase)
4. Refactor: extract `load_file(path) -> anyhow::Result<Option<AppConfig>>` helper

## Acceptance Criteria

1. **Defaults When No Config**
   - Given no `ap.toml` and no `~/.ap/config.toml` exist
   - When calling `AppConfig::load()`
   - Then returns `Ok(AppConfig)` with model = `us.anthropic.claude-sonnet-4-6`, backend = `bedrock`, region = `us-west-2`

2. **Project Config Loads**
   - Given a valid `ap.toml` with `[provider] model = "custom-model"` in the current directory
   - When calling `AppConfig::load()`
   - Then returns `Ok(AppConfig)` with `provider.model = "custom-model"`

3. **Project Overrides Global**
   - Given a global config with `model = "global-model"` and project config with `model = "project-model"`
   - When calling `AppConfig::load()`
   - Then `provider.model` is `"project-model"`

4. **Invalid TOML Returns Error With Path**
   - Given an `ap.toml` containing `[invalid toml %%%`
   - When calling `AppConfig::load()`
   - Then returns `Err` and the error message contains the file path

5. **All 5 Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test config`
   - Then all 5 config tests pass with zero failures

## Metadata
- **Complexity**: Low
- **Labels**: config, serde, toml
- **Required Skills**: Rust, serde, TOML parsing
