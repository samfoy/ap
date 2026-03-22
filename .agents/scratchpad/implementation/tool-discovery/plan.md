# Tool Discovery — Implementation Plan

*Author: Ralph (Planner hat) | Date: 2026-03-22*

---

## Test Strategy

### Unit Tests

#### Step 1: `src/discovery/mod.rs` — Types + Serde

| Test | Inputs | Expected |
|---|---|---|
| `parses_valid_tools_toml` | valid `[[tool]]` TOML string | `ToolsFile` with correct name/description/command/params |
| `parses_valid_skill_toml` | `[[tool]]` + `system_prompt` string | `SkillFile` with `system_prompt = Some(...)` |
| `param_required_defaults_to_true` | param TOML without `required` field | `ParamSpec { required: true }` |
| `param_required_false_explicit` | `required = false` in TOML | `ParamSpec { required: false }` |
| `tools_file_empty_no_tool_sections` | TOML with no `[[tool]]` | `ToolsFile { tools: vec![] }` (not an error) |

#### Step 2: `src/discovery/mod.rs` — `discover()` function

All tests use `tempfile::TempDir`.

| Test | Setup | Expected |
|---|---|---|
| `discover_empty_dir` | Empty TempDir | `DiscoveryResult { tools: [], additions: [], warnings: [] }` |
| `discover_valid_tools_toml` | Write valid `tools.toml` with 2 tools | 2 `DiscoveredTool`s, 0 warnings |
| `discover_tools_toml_fields` | Single tool with all fields | Check name/description/command/params match |
| `discover_malformed_tools_toml` | Broken TOML | 0 tools, 1 warning containing "tools.toml" |
| `discover_malformed_tool_entry` | Valid TOML but tool missing required field | 0 tools from file, 1 warning (whole file skip) |
| `discover_skills_dir` | `.ap/skills/ci.toml` with 1 tool + system_prompt | 1 tool, 1 addition, 0 warnings |
| `discover_malformed_skill_file` | `.ap/skills/bad.toml` with broken TOML | 0 tools from that file, 1 warning |
| `discover_system_prompt_accumulates` | 2 skill files each with `system_prompt` | 2 additions in result |
| `discover_required_false_param` | Tool with `required = false` param | Param in `params`, not in schema `required` array |
| `discover_duplicate_name_tools_toml_wins` | Same tool name in `tools.toml` and skill file | First definition (tools.toml) kept, warning for skill file |
| `discover_skills_alphabetical_order` | `b.toml` with "build", `a.toml` with "lint" | No collision; both registered; a.toml processed first |
| `discover_duplicate_name_two_skills` | Same name in `a.toml` and `b.toml` | `a.toml` wins (alphabetically first), warning for `b.toml` |
| `discover_read_dir_sorted` | `.ap/skills/z.toml` (earlier in dir, but name after a.toml) | Alphabetical sort ensures determinism |
| `discover_param_insertion_order` | Tool with params c, a, b in TOML order | IndexMap preserves c→a→b order |

#### Step 3: `src/tools/shell.rs` — `ShellTool`

| Test | Setup | Expected |
|---|---|---|
| `schema_required_params_in_required_array` | Tool with 2 required, 1 optional param | Schema `required` contains only 2 required param names |
| `schema_optional_params_in_properties_not_required` | Tool with `required = false` param | Param appears in `properties` but not `required` |
| `execute_with_all_required_params` | `echo $AP_PARAM_FOO` command, `foo=bar` | Output contains "bar" |
| `execute_env_var_uppercased` | `echo $AP_PARAM_MY_KEY` command, `my_key=hello` | Output contains "hello" |
| `execute_missing_required_param` | Tool requires `foo`, call with empty params | `ToolResult` is error, message contains "missing required parameter: foo" |
| `execute_optional_param_absent` | `echo ${AP_PARAM_OPT:-default}` command, no opt | Command succeeds, uses default |
| `execute_exit_nonzero_not_error` | `exit 1` command | `ToolResult` is ok (not err), content contains "exit: 1" |
| `execute_command_spawn_failure` | Nonsense command path | `ToolResult` is error containing "failed to spawn" |
| `execute_runs_in_root_dir` | `pwd` command | Output matches the `root` PathBuf |

#### Step 4: System Prompt Threading

| Test | Location | Expected |
|---|---|---|
| `bedrock_build_request_body_with_system_prompt` | `bedrock.rs` unit test | JSON body contains `"system": "my prompt"` key |
| `bedrock_build_request_body_no_system_prompt` | `bedrock.rs` unit test | JSON body has NO `"system"` key |
| `conversation_with_system_prompt_builder` | `types.rs` unit test | `conv.system_prompt == Some("test".to_string())` after builder call |
| `conversation_serde_backward_compat` | `types.rs` unit test | Old JSON without `system_prompt` field deserializes without error |

### Integration Tests

#### Step 4: All MockProvider impls

The compiler enforces these automatically. `cargo check` after Step 4 must pass clean.

- `ap/src/turn.rs` inline `MockProvider::stream_completion` — add `_system_prompt: Option<&'a str>` param
- `ap/tests/noninteractive.rs` `MockProvider::stream_completion` — same update

### Regression

After every step: `cargo test --package ap` must pass. All existing tests are regression tests.

---

## E2E Scenario (Manual — for Validator)

**Harness:** Real CLI in a temp project directory.

### Setup

```bash
cd /tmp && mkdir ap-e2e-test && cd ap-e2e-test
git init
cat > tools.toml <<'EOF'
[[tool]]
name = "greet"
description = "Greet the user"
command = "echo Hello, $AP_PARAM_NAME!"

[tool.params.name]
description = "Name to greet"
required = true
EOF

mkdir -p .ap/skills
cat > .ap/skills/dev.toml <<'EOF'
system_prompt = "This project uses a greeting tool. Always greet users by name."

[[tool]]
name = "farewell"
description = "Say goodbye"
command = "echo Goodbye, $AP_PARAM_NAME!"

[tool.params.name]
description = "Name to bid farewell"
required = true
EOF
```

### Happy Path

```bash
# Run ap and ask Claude to use the greet tool
ap --no-tui
# Type: "Please use the greet tool with my name as 'World'"
# Expected: Claude calls greet("World"), output shows "Hello, World!"
```

**Observable outcomes:**
1. No warnings printed to stderr on startup
2. Claude's tool list includes "greet" and "farewell" (visible in initial message schema)
3. Claude can invoke `greet` with `name=World` and the output contains "Hello, World!"
4. System prompt context ("This project uses a greeting tool...") influences Claude's response framing

### Adversarial Paths

1. **Missing required param**: Ask Claude to call `greet` without providing a name.
   - Expected: `ToolResult` error "missing required parameter: name" appears in conversation

2. **Duplicate tool name conflict**: Add `farewell` to `tools.toml` too.
   - Expected: `ap: tool 'farewell' in .ap/skills/dev.toml conflicts with earlier definition — skipped` printed to stderr
   - Only one `farewell` tool registered

3. **Malformed skill file**: Break `.ap/skills/dev.toml` (remove a `=`).
   - Expected: `ap: .ap/skills/dev.toml: ...` warning on stderr, ap still starts, `greet` from `tools.toml` still available

---

## Implementation Steps

### Step 1: Discovery types + TOML serde

**Files:**
- `ap/Cargo.toml` — add `indexmap = { version = "2", features = ["serde"] }`
- `ap/src/lib.rs` — add `pub mod discovery;`
- `ap/src/discovery/mod.rs` — new file: `DiscoveryResult`, `DiscoveredTool`, `ParamSpec`, private `ToolsFile`/`SkillFile`/`RawTool` structs

**Tests that must pass:** `parses_valid_tools_toml`, `parses_valid_skill_toml`, `param_required_defaults_to_true`, `param_required_false_explicit`, `tools_file_empty_no_tool_sections`

**Build gate:** `cargo check --package ap` passes clean.

**Demo:** `cargo test --package ap discovery::` runs the serde tests.

---

### Step 2: `discover()` function

**Files:**
- `ap/src/discovery/mod.rs` — add `pub fn discover(root: &Path) -> DiscoveryResult`

**Tests that must pass:** All `discover_*` tests from the unit test table above (12 tests).

Key implementation notes:
- `std::fs::read_to_string(root.join("tools.toml"))` — missing file is `Ok("")`... actually returns `Err(NotFound)` → handle with `match`
- Skills: `std::fs::read_dir(root.join(".ap/skills"))` → collect to `Vec`, sort by `file_name()`, iterate
- Maintain `HashSet<String> seen` across all files
- Each file: `toml::from_str::<ToolsFile/SkillFile>(&content)` — on `Err`, push warning and `continue`
- On success: iterate `raw_tools`, check `seen`, push to result or push warning

**Build gate:** `cargo test --package ap discovery::` — all tests pass.

**Demo:** Unit tests demonstrate pure parsing with temp dirs.

---

### Step 3: `ShellTool`

**Files:**
- `ap/src/tools/shell.rs` — new file: `ShellTool` implementing `Tool`
- `ap/src/tools/mod.rs` — add `pub mod shell;` + `pub use shell::ShellTool;`

**Tests that must pass:** All `execute_*` and `schema_*` tests from unit test table (9 tests).

Key implementation notes:
- `use crate::discovery::DiscoveredTool;`
- Schema: iterate `tool.params`, collect required keys, build `json!({...})` matching the spec
- Execute: validate required params first (return err if any missing), then spawn `std::process::Command::new("sh").arg("-c").arg(&self.tool.command)`, inject env vars `AP_PARAM_{KEY}`, set `.current_dir(&self.root)`, collect stdout/stderr, return ok with combined output
- Async: `Box::pin(async move { ... })` wrapping sync `Command::output()`

**Build gate:** `cargo test --package ap tools::shell::` — all tests pass. `cargo check` passes.

**Demo:** Unit tests show tool schema + execution with env var injection.

---

### Step 4: System prompt threading

**Files:**
- `ap/src/types.rs` — add `system_prompt: Option<String>` field + `with_system_prompt()` builder
- `ap/src/provider/mod.rs` — update `Provider` trait signature
- `ap/src/provider/bedrock.rs` — update `BedrockProvider` impl + `build_request_body`
- `ap/src/turn.rs` — update `turn_loop` call site + inline `MockProvider`
- `ap/tests/noninteractive.rs` — update `MockProvider`

**Tests that must pass:**
- `bedrock_build_request_body_with_system_prompt` — body has `"system"` key
- `bedrock_build_request_body_no_system_prompt` — body has no `"system"` key
- `conversation_with_system_prompt_builder` — builder works
- `conversation_serde_backward_compat` — old JSON deserializes fine
- All existing tests still pass (compiler-enforced + runtime)

**Build gate:** `cargo test --package ap` — all tests pass (existing + new).

**Demo:** Bedrock unit tests verify the `"system"` field is conditionally present.

---

### Step 5: Wire `main.rs`

**Files:**
- `ap/src/main.rs` — both `run_headless` and `run_tui`

**Changes in each function:**
1. Get `project_root` via `std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))`
2. Call `discover(&project_root)`, print warnings to stderr
3. Build `ToolRegistry::with_defaults()` then register `ShellTool`s **before** `Arc::new` wrap
4. Build system prompt from `system_prompt_additions`, call `conv.with_system_prompt()` if non-empty

**Build gate:** `cargo build --package ap` passes. `cargo test --package ap` passes.

**Demo:** Run `ap` in a project directory with `tools.toml` — new tools appear in Claude's context.

---

## Success Criteria (from acceptance tests)

1. `DiscoveredTool` serialises/deserialises via TOML serde
2. `discover()` returns empty result for empty dir (no panics)
3. `discover()` parses `tools.toml` with one `[[tool]]` entry correctly
4. `discover()` accumulates warnings for malformed files without aborting
5. `discover()` deduplicates tool names with first-wins + warning
6. `ShellTool::execute` injects `AP_PARAM_*` env vars
7. `ShellTool::execute` returns error for missing required param
8. `ShellTool::schema` puts required params in `"required"` array only
9. `Conversation::with_system_prompt` sets the field, `#[serde(default)]` ensures compat
10. `Provider::stream_completion` signature has `system_prompt: Option<&'a str>`
11. `BedrockProvider` includes `"system"` in request body when `Some`, omits when `None`
12. `turn()` passes `conv.system_prompt.as_deref()` to provider
13. `discover()` reads `.ap/skills/*.toml` alphabetically
14. `discover()` extracts `system_prompt` from skill files into `system_prompt_additions`
15. `cargo clippy -- -D warnings` passes (no unwrap outside tests)
