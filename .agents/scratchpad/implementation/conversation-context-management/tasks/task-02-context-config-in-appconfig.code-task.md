---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: ContextConfig in AppConfig

## Description
Add a `ContextConfig` struct to `ap/src/config.rs` and integrate it into `AppConfig`. This gives the application a first-class configuration surface for context-limit settings that persists in TOML config files and supports `#[serde(default)]` so old config files without the `[context]` table continue to deserialize correctly.

## Background
The existing `AppConfig` has a `skills: SkillsConfig` field that uses `#[serde(skip)]`. The new `ContextConfig` is different: it MUST be serialized/deserialized so that per-project limits set in `ap.toml` are respected across sessions. The `overlay_from_table` method in `AppConfig` handles TOML table merging and must be extended to handle the new `[context]` table key.

## Reference Documentation
**Required:**
- Design: `.agents/scratchpad/implementation/conversation-context-management/design.md`

**Additional References:**
- `.agents/scratchpad/implementation/conversation-context-management/context.md` (codebase patterns, especially config.rs overlay pattern)
- `.agents/scratchpad/implementation/conversation-context-management/plan.md` (overall strategy)

**Note:** You MUST read the design document before beginning implementation. Also read the existing `ap/src/config.rs` in full before making changes.

## Technical Requirements
1. Add `ContextConfig` struct with `#[derive(Debug, Clone, Serialize, Deserialize, Default)]`:
   - `pub limit: Option<u32>` with `#[serde(default)]` — `None` by default
   - `pub keep_recent: usize` with `#[serde(default = "default_keep_recent")]` — default `20`
   - `pub threshold: f32` with `#[serde(default = "default_threshold")]` — default `0.80`
2. Add `pub context: ContextConfig` to `AppConfig` with `#[serde(default)]`
3. Extend `overlay_from_table` to handle the `"context"` key by deserializing the sub-table into `ContextConfig` and merging it
4. All 5 new tests must pass; all existing `config::tests` must continue to pass

## Dependencies
- Task 01 (context.rs module exists, though not directly called here)

## Implementation Approach
1. **TDD: Write all 5 failing tests first** in the `#[cfg(test)]` block in `config.rs`
2. Add `ContextConfig` struct above `AppConfig`
3. Add the `context` field to `AppConfig`
4. Extend `overlay_from_table`
5. `cargo test config::tests` — all tests (old + new) must pass
6. `cargo build` — zero warnings

## Acceptance Criteria

1. **context_config_defaults**
   - Given `ContextConfig::default()`
   - When inspecting the result
   - Then `limit == None`, `keep_recent == 20`, `threshold == 0.80`

2. **context_config_toml_limit**
   - Given a TOML string `[context]\nlimit = 100000`
   - When deserializing into `AppConfig`
   - Then `config.context.limit == Some(100000)`, `keep_recent` and `threshold` remain at defaults

3. **context_config_toml_full**
   - Given a TOML string with all three keys (`limit = 50000`, `keep_recent = 10`, `threshold = 0.75`)
   - When deserializing into `AppConfig`
   - Then all three values are parsed correctly

4. **context_config_missing_keys_preserve_defaults**
   - Given a TOML string with only `limit = 200000` in the `[context]` table
   - When deserializing into `AppConfig`
   - Then `limit == Some(200000)`, `keep_recent == 20`, `threshold == 0.80`

5. **context_config_no_auto_summarize_when_limit_none**
   - Given `AppConfig::default()`
   - When inspecting `config.context.limit`
   - Then it equals `None`

6. **All Existing Config Tests Pass**
   - Given the implementation is complete
   - When running `cargo test config::tests`
   - Then all pre-existing tests continue to pass

## Metadata
- **Complexity**: Low
- **Labels**: context-management, config, serde, tdd
- **Required Skills**: Rust, serde, TOML deserialization
