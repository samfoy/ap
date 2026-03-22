# Existing Patterns — skill-system

All file:line references are from `ap/src/`.

---

## 1. Builder pattern (consuming self)

`Conversation::with_user_message` (`types.rs:39`) uses `mut self` → returns `Self`:
```rust
pub fn with_user_message(mut self, content: impl Into<String>) -> Self {
    self.messages.push(...);
    self
}
```
`with_system_prompt` must follow the same shape.

`Middleware` builder methods in `middleware.rs:16–43` also use `mut self` returning `Self`:
```rust
pub fn pre_turn(mut self, f: impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static) -> Self {
    self.pre_turn.push(Box::new(f) as TurnMiddlewareFn);
    self
}
```
The skill closure signature must match `TurnMiddlewareFn` = `Box<dyn Fn(&Conversation) -> Option<Conversation> + Send + Sync>` (types.rs:97).

---

## 2. Config struct pattern

All config structs use:
- `#[derive(Debug, Clone, Serialize, Deserialize)]`
- `#[serde(default)]`
- Manual `Default` impl (or `derive(Default)` for simple cases)

`ProviderConfig` (`config.rs:10`) → manual Default with specific values.
`HooksConfig` (`config.rs:22`) → `derive(Default)` (all fields `Option`).
`ToolsConfig` (`config.rs:28`) → manual Default with `vec!["read","write","edit","bash"]`.
`AppConfig` (`config.rs:40`) → `derive(Default)` (delegates to sub-config defaults).

`SkillsConfig` will need manual `Default` (non-trivial defaults: `enabled=true`, `max_injected=5`).

---

## 3. Config overlay pattern

`overlay_from_table` (`config.rs:52`) handles each top-level TOML key:
1. Extract as `toml::Value::Table`
2. `try_into::<SubConfig>()` 
3. For each field: `if table.contains_key("field")` → set `base.sub.field`

Must add a `[skills]` block following the same pattern.

---

## 4. Test patterns

- Module-level `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` in every test module
- `#[cfg(test)]` block at bottom of each file
- Helper `dummy_config()` / `make_conv()` functions
- No `async_std` — all async tests use `#[tokio::test]`

---

## 5. Provider trait call sites

All places that call `provider.stream_completion(...)` or implement the `Provider` trait:

| File | Location | Action needed |
|------|----------|---------------|
| `turn.rs:74` | `provider.stream_completion(&messages_snapshot, &tool_schemas)` | Add `.as_deref()` from `conv.system_prompt` |
| `turn.rs:245` | `MockProvider::stream_completion` impl | Update signature |
| `turn.rs:263` | `ErrorProvider::stream_completion` impl | Update signature |
| `provider/bedrock.rs:148` | `BedrockProvider::stream_completion` impl | Update signature + pass through |
| `provider/bedrock.rs:92` | `fn build_request_body(...)` | Add `system_prompt: Option<&str>` param + inject |

---

## 6. serde skip pattern

`Conversation` currently does NOT have any `#[serde(skip)]` fields — the `system_prompt` field will be the first. The pattern is standard serde:
```rust
#[serde(skip)]
pub system_prompt: Option<String>,
```
With `#[serde(skip)]`, the field is excluded from both serialization and deserialization. It will be initialized to `Default::default()` (i.e., `None`) on deserialisation.

---

## 7. Error handling for I/O in skills

`SkillLoader::load()` must silently skip non-existent dirs and warn on unreadable files. The codebase uses `eprintln!` for warnings in non-critical paths (see `main.rs:38,45,61,106,121`). No `tracing` crate is present — use `eprintln!`.

---

## 8. dirs crate (v5)

`dirs = "5"` is present in `Cargo.toml:44`. API:
```rust
dirs::home_dir() -> Option<PathBuf>
```
Used in `config.rs:106`:
```rust
dirs::home_dir().map(|h| h.join(".ap").join("config.toml"))
```
Same pattern for skills dirs.

---

## 9. `lib.rs` module registration

`lib.rs` has 9 `pub mod` declarations. Adding `pub mod skills;` follows the same pattern.

---

## 10. Clippy deny rules (main.rs:1–5)

```rust
#![deny(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
```

These apply to `main.rs` only via file-level attributes. The library crate has the same rules in `Cargo.toml:lints`. All production code must use `?` / `unwrap_or_else` / `map_err`. Tests get the allow exemption.

---

## 11. `tempfile` crate

`tempfile = "3"` appears in BOTH `[dependencies]` AND `[dev-dependencies]`. Already available for integration tests.

---

## 12. Integration test location

No `tests/` directory currently exists in `ap/`. The design spec calls for `tests/skill_injection.rs`. This will be the first integration test file.
