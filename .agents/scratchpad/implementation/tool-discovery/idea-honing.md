# Tool Discovery — Idea Honing

## Q1 (2026-03-22) — Error handling in `discover()`

The spec defines `discover(root: &Path) -> DiscoveryResult` as a pure function (no global I/O).

When `discover()` encounters a malformed `.ap/skills/*.toml` file (bad TOML, missing required fields), what should it do?

- **Option A:** Silently skip the bad file and continue (best-effort discovery)
- **Option B:** Return a `Result<DiscoveryResult, DiscoveryError>`, surfacing the parse failure to the caller
- **Option C:** Include a `Vec<String>` of warnings/errors in `DiscoveryResult` so callers can decide whether to surface them

This affects the function signature, `DiscoveryResult` shape, and whether `main.rs` can treat discovery as infallible.

---

**Answer: Option C — `Vec<String>` warnings in `DiscoveryResult`.**

**Rationale (grounded in codebase patterns):**

The existing codebase has a strong "warn but continue" philosophy. `main.rs` calls `AppConfig::load().unwrap_or_default()` — config failures are non-fatal. Session loading uses `unwrap_or_else(|e| { eprintln!(...); fallback })`. Tool discovery should match this pattern.

- **Option A** (silent skip) is rejected: a user whose skill file silently disappears has no way to know why a tool isn't showing up. Debugging becomes opaque.
- **Option B** (Result) is rejected: it makes discovery all-or-nothing. One malformed file in `.ap/skills/` would abort the entire startup — that's too harsh for a best-effort, additive feature. It also forces `main.rs` to use `?` or `unwrap` on something that should never be fatal.
- **Option C** (warnings in result) is the right fit:
  - `discover()` remains infallible — `main.rs` calls it without error handling ceremony.
  - Each bad file contributes a human-readable warning string (e.g., `"warning: .ap/skills/bad.toml: missing required field 'command'"`) that `main.rs` can print via `eprintln!`.
  - Successfully parsed tools from other files still load.
  - Consistent with the "warn but continue" pattern throughout `main.rs`.

**Resulting shape:**

```rust
pub struct DiscoveryResult {
    pub tools: Vec<DiscoveredTool>,
    pub system_prompt_additions: Vec<String>,
    pub warnings: Vec<String>,   // ← added
}
```

`main.rs` startup:
```rust
let discovery = discover(&project_root);
for w in &discovery.warnings {
    eprintln!("ap: {w}");
}
```

*Answer recorded 2026-03-22*

---

## Q2 (2026-03-22) — `ParamSpec` fields

The spec defines `params: IndexMap<String, ParamSpec>` in `DiscoveredTool`, but `ParamSpec` itself is not defined.

This is critical for:
1. **JSON schema generation** — `ShellTool::schema()` must emit `"properties"` and `"required"` arrays for Claude to call the tool correctly
2. **Env var injection** — how does `ShellTool` know which params are required vs optional?
3. **TOML format** — what fields appear under `[tool.params.param_name]`?

Concretely, for a `tools.toml` like:
```toml
[[tool]]
name = "run-tests"
command = "cargo test $AP_PARAM_FILTER"
[tool.params.filter]
description = "Test filter glob"
# required = true?  type = "string"?
```

What should `ParamSpec` contain?

- **Option A — description only**: `ParamSpec { description: String }` — all params are always required strings (simplest; ShellTool lists every param in `"required"`)
- **Option B — description + required flag**: `ParamSpec { description: String, required: bool }` — tools can have optional params; ShellTool only lists required ones in the JSON schema
- **Option C — description + required + type enum**: `ParamSpec { description: String, required: bool, type: ParamType }` where `ParamType` is `String | Number | Boolean` — richer schema for Claude; more complex serde

---

**Answer: Option B — `description + required` flag.**

**Rationale:**

1. **`type` is unnecessary for ShellTool**: env vars are always strings at the OS level. `AP_PARAM_COUNT=5` is a string regardless of what type schema says. A `ParamType` enum adds serde complexity and a `match` arm in `ShellTool::schema()` but changes zero bytes of actual execution behaviour.

2. **`required` is load-bearing**: without it, `ShellTool` cannot distinguish "skip this env var if not provided" from "fail with missing parameter". With `required: bool`, the execution path is clear:
   - `required: true` + param absent → return `ToolResult::err("missing required parameter: foo")`
   - `required: false` + param absent → simply don't set `AP_PARAM_FOO` (command handles the empty case)

3. **JSON schema correctness matters**: Claude uses the `"required"` array in the tool schema to decide which params it must always supply. Without a `required` flag, `ShellTool::schema()` would have to list every param as required (Option A's behaviour), breaking optional-param tools.

4. **Consistent with existing built-in tools**: `BashTool`, `ReadTool`, etc. all emit `"required": ["command"]` / `"required": ["path"]` in their schemas. Discovered tools should follow the same pattern.

5. **Simplest design that meets requirements**: `type` can be added later as a backward-compatible TOML field. For now, `String` is the only type that matters for shell-based tools.

**Resulting struct:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSpec {
    pub description: String,
    #[serde(default)]          // defaults to true if omitted from TOML
    pub required: bool,
}
```

Default for `required` is `true` — omitting it in TOML treats the param as required, which is the safe/strict default. Authors opt into optional with `required = false`.

**TOML example:**

```toml
[[tool]]
name = "run-tests"
command = "cargo test $AP_PARAM_FILTER"

[tool.params.filter]
description = "Test filter glob (optional)"
required = false

[[tool]]
name = "deploy"
command = "deploy.sh $AP_PARAM_ENV"

[tool.params.env]
description = "Target environment (staging or production)"
# required = true is the default; omitting is fine
```

*Answer recorded 2026-03-22*



---

---

## Post-Design-Rejection Q&A (2026-03-22)

Design Critic rejected the design for two issues:
1. Skip-file vs skip-tool contradiction — serde's `Vec<T>` fails the entire Vec when any element fails to deserialize; design contradicted FR-2 (skip whole file) without addressing this constraint
2. Duplicate tool name collision across files — undefined behavior in ToolRegistry

### Question 3 (2026-03-22)

The design has a contradiction: FR-2 says "skip the whole file" when a `[[tool]]` entry is malformed, but the design also suggested "skip individual tool, add warning." These are mutually exclusive, and there's a real technical constraint: serde deserializes `Vec<DiscoveredTool>` all-or-nothing — if any `[[tool]]` entry is invalid, the whole Vec parse fails.

**Two implementation paths:**
- **Path A (skip whole file):** Deserialize `struct ToolsFile { tools: Vec<DiscoveredTool> }` normally — any bad entry fails the whole file, add one warning for the file, move on. Simple code.
- **Path B (skip individual tools):** Deserialize as `Vec<toml::Value>` first, then iterate and try parsing each entry individually — bad entries get their own warning, good ones in the same file are still registered. More complex code (~15 extra lines).

**Question:** When a `tools.toml` or skill file contains one malformed `[[tool]]` entry alongside valid ones, should `discover()` skip the **entire file** (Path A — simpler) or skip only the **bad entry** and keep the valid ones (Path B — more granular)?

### Answer to Question 3 (2026-03-22)

**Decision: Path A — skip the entire file.**

**Reasoning:**

1. **FR-2 already mandates this.** The original requirement says "On malformed TOML or missing required fields: add a warning string, skip that file." Path A is the spec-compliant default.

2. **Serde gives Path A for free.** Deserializing `struct ToolsFile { tools: Vec<DiscoveredTool> }` — if any `[[tool]]` entry fails, the entire parse fails. That's zero extra code to implement Path A.

3. **Path B costs more than it saves.** The ~15 extra lines of custom `Vec<toml::Value>` deserialization adds a maintenance surface, two-phase parsing complexity, and a different error model for the same user scenario (a misconfigured tools file). The user still sees a warning and must fix the file — partial registration doesn't help them meaningfully.

4. **Tools files are small.** A `tools.toml` or skill file typically has <10 tools. Losing all of them because of one bad entry is a visible, actionable error. Silently losing one tool while registering others would be harder to notice.

**Implementation:** Deserialize normally with serde; catch any `toml::de::Error` at the file level, push a formatted warning (`"{filename}: {error}"`), and continue to the next file. No custom deserialization needed.

---

---

## Question 4 (2026-03-22) — Tool name collision across files

The Design Critic flagged that `ToolRegistry` appends tools without deduplication. If `tools.toml` and a `.ap/skills/*.toml` file both define a tool with the same name (e.g., `"build"`), the current `find_by_name` returns the first match and `all_schemas()` returns duplicates — two identical schemas sent to Claude.

**Three resolution strategies:**
- **Last-wins:** Later-loaded tool (skills file) overwrites earlier one (tools.toml). Skills can override project defaults.
- **First-wins:** Earlier-loaded tool (tools.toml) takes precedence. Project definitions can't be accidentally overridden by skill packs.
- **Warn-and-skip-duplicate:** Second definition of any name is rejected; a warning is added to `DiscoveryResult.warnings`. Both names remain unique; user must resolve conflict explicitly.

**Question:** When two files define a tool with the same name, should `discover()` use last-wins, first-wins, or warn-and-skip the duplicate?

### Answer to Question 4 (2026-03-22)

**Decision: warn-and-skip-duplicate (first-wins with warning).**

**Reasoning:**

1. **Deterministic load order gives first-wins natural semantics.** `discover()` loads `tools.toml` first, then `.ap/skills/*.toml` (alphabetically sorted). Project-local tools in `tools.toml` are more specific than generic skill packs, so first-wins correctly expresses "local definitions take precedence over skill packs."

2. **No silent shadowing.** Bare first-wins (no warning) would silently discard a skill tool that collides with a project tool. The user has no way to know a skill definition was ignored. With a warning in `DiscoveryResult.warnings`, startup prints something like: `ap: warning: tool 'build' defined in both tools.toml and .ap/skills/rust.toml — using definition from tools.toml`. The user can rename to resolve.

3. **No `ToolRegistry` changes needed.** The deduplication happens entirely inside `discover()` using a `HashSet<String>` of already-seen tool names as the Vec is built. The returned `DiscoveryResult.tools` is pre-deduplicated before any registration happens. `ToolRegistry` stays as-is.

4. **Claude sees clean schemas.** `all_schemas()` will never return duplicate schemas, so Claude's tool list is unambiguous.

**Implementation sketch (in `discover()`):**
```rust
let mut seen: HashSet<String> = HashSet::new();
let mut tools: Vec<DiscoveredTool> = Vec::new();

for tool in parsed_tools {
    if seen.contains(&tool.name) {
        warnings.push(format!("tool '{}' in {} conflicts with earlier definition — skipped", tool.name, filename));
        continue;
    }
    seen.insert(tool.name.clone());
    tools.push(tool);
}
```

Load order determines precedence:
1. `tools.toml` (project root — highest precedence)
2. `.ap/skills/*.toml` sorted alphabetically (lower precedence)

*Answer recorded 2026-03-22*

---

## Requirements Complete (2026-03-22)

All critical questions answered. Summary of decisions:

1. **`discover()` error handling**: Option C — `warnings: Vec<String>` in `DiscoveryResult`. Infallible function, best-effort loading, human-readable warnings printed via `eprintln!` at startup.

2. **`ParamSpec` shape**: Option B — `{ description: String, required: bool }`. `#[serde(default)]` for `required` defaults to `true` (strict/safe). No type enum needed — env vars are always strings at OS level.

### Final `DiscoveryResult` shape:
```rust
pub struct DiscoveryResult {
    pub tools: Vec<DiscoveredTool>,
    pub system_prompt_additions: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct ParamSpec {
    pub description: String,
    #[serde(default = "default_required")]
    pub required: bool,
}
```

Remaining implementation details derivable from spec:
- ShellTool runs from project root (the `root` param to `discover()`)
- `AP_PARAM_*` naming: param key uppercased, e.g. `filter` → `AP_PARAM_FILTER`
- Multiple `system_prompt_additions` joined with `\n\n` in `main.rs`
