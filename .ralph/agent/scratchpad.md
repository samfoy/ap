# Scratchpad

## 2026-03-22 — Tool Discovery Feature

### Objective
Implement tool discovery for `ap` so Claude gets named, schematised tools from project context instead of raw bash guessing.

### Codebase State
- Project is at `./ap/` (Rust, Tokio async)
- Current architecture: `types.rs` (Conversation, TurnEvent), `provider/mod.rs` (Provider trait), `turn.rs` (pure turn()), `tools/mod.rs` (Tool trait + ToolRegistry)
- `Conversation` has: id, model, messages, config — no system_prompt yet
- `Provider::stream_completion` takes: messages, tools — no system_prompt yet
- `tempfile` crate already in dev-dependencies (also in main deps)
- `toml` crate already available for TOML parsing

### What Needs to Be Built (5 Steps)
1. **Discovery types** — `DiscoveredTool` struct + `DiscoveryResult` with TOML serde in `ap/src/discovery/mod.rs`
2. **discover() function** — pure fn scanning `tools.toml` + `.ap/skills/*.toml`, tempfile-based tests
3. **ShellTool** — implements `Tool` trait, runs command via bash, injects `AP_PARAM_*` env vars
4. **System prompt threading** — `Conversation::system_prompt` field + `with_system_prompt()`, update `Provider::stream_completion` signature, thread through `turn()`
5. **main.rs wiring** — call `discover()` at startup, build system prompt, register ShellTools

### Key Types from Spec
- `DiscoveredTool`: name, description, params (IndexMap<String, ParamSpec>), command
- `DiscoveryResult`: tools: Vec<DiscoveredTool>, system_prompt_additions: Vec<String>
- `discover(root: &Path) -> DiscoveryResult` (pure, no global I/O)

### TOML Formats
- `tools.toml`: [[tool]] array with name, description, command, [tool.params] table
- `.ap/skills/*.toml`: same format with optional system_prompt string

### System Prompt Threading
- `Conversation` gains `system_prompt: Option<String>` + `with_system_prompt()`
- `Provider::stream_completion` gains `system_prompt: Option<&'a str>` param
- BedrockProvider passes it to the `system` field of the Bedrock API call
- `turn()` extracts it from `conv.system_prompt` and passes to provider

### Plan
- Emit design.start to kick off the workflow
- The spec is detailed enough to go quickly through the pipeline

## 2026-03-22 — Design Complete

Synthesized all Q&A into requirements.md + design.md in `.agents/scratchpad/implementation/tool-discovery/`.

### Key design decisions documented:
- `discover()` is infallible; warnings accumulate in `DiscoveryResult.warnings`
- `ParamSpec` = `{ description: String, required: bool }` with `serde(default)` for `required = true`
- `indexmap` crate needed (not yet in Cargo.toml) for insertion-order params
- `ShellTool` runs via `sh -c` with `AP_PARAM_*` env vars, same format as BashTool
- `Conversation.system_prompt: Option<String>` + `with_system_prompt()` builder
- `Provider::stream_completion` gains `system_prompt: Option<&'a str>` param
- `BedrockProvider` adds `"system"` key to request body when Some
- `turn()` extracts `conv.system_prompt.as_deref()` and forwards to provider
- 5 ordered impl steps, each independently compilable

### Files written:
- `.agents/scratchpad/implementation/tool-discovery/requirements.md`
- `.agents/scratchpad/implementation/tool-discovery/design.md`

Emitting `design.drafted` → Design Critic receives it next.

## 2026-03-22 — Design Critic Review

### Review findings:

**FAIL — Error handling granularity inconsistency (Section 5 vs FR-2)**

Section 5 has an unresolved contradiction:
- Table column says: "Warning + skip tool (not whole file)"
- Same row notes: "— actually: skip whole file per the spec"
- Note at bottom of Section 5 reverses again: "Individual tool parse failures within a file should skip that tool and add a warning"
- FR-2 says: "On malformed TOML or missing required fields: add a warning string, **skip that file**"

The design cannot implement both. Worse: serde's `Vec<T>` deserialization fails the entire Vec when any element fails to deserialize — making "skip individual tool" non-trivial without custom deserialization. The design does not address this technical constraint.

**CONCERN — Tool name collision behavior undefined**

`ToolRegistry::register` appends without dedup. `find_by_name` returns first match. `all_schemas()` returns all including duplicates. If `tools.toml` and a skill file both define a tool named "build", the result is:
- Two identical schemas sent to Claude (confusing)
- `find_by_name("build")` picks the first one (silent shadowing)

The design doesn't specify: last-wins, first-wins, or warn-and-skip-duplicate. This is an edge case that will happen in practice (e.g., a skill pack that overlaps a local tools.toml entry).

### Verdict: REJECT

The first issue is a genuine FAIL — the design contradicts FR-2 and ignores a serde implementation constraint. The second is a real-world correctness gap. Need the Inquisitor to resolve both before implementation.

## 2026-03-22 — Q4 Answer: Tool Name Collision

**Decision: warn-and-skip-duplicate (first-wins with warning)**

- `discover()` tracks seen names in a `HashSet<String>` while building `DiscoveryResult.tools`
- Load order: `tools.toml` first, then `.ap/skills/*.toml` alphabetically → local tools win over skills
- Duplicate name → push warning to `DiscoveryResult.warnings`, skip the tool
- `ToolRegistry` requires zero changes; dedup is entirely in `discover()`
- Clean schemas sent to Claude (no duplicates in `all_schemas()`)

This resolves the second Design Critic FAIL. With Q3 (skip whole file) and Q4 (warn-and-skip-duplicate) both answered, all design gaps are closed. Inquisitor should now close requirements.

## 2026-03-22 — Requirements Complete (post Q4)

Q4 answer (warn-and-skip-duplicate) recorded in idea-honing.md. All design critic gaps now closed:
- Q3: skip whole file (serde gives this for free; FR-2 mandated it)
- Q4: warn-and-skip-duplicate first-wins; HashSet in discover(); ToolRegistry unchanged

Task `pdd:tool-discovery:requirements` closed. Emitting requirements.complete → Architect next.

## 2026-03-22 — Design Revised (post Design Critic rejection)

Updated design.md to resolve both Design Critic FAILs:

1. **Section 5 (Error Handling)**: Removed the contradictory "skip tool vs skip file" note. Table now clearly states "skip entire file" for both `tools.toml` and skill files. Added rationale: serde's `Vec<T>` gives whole-file failure for free; no custom deserializer needed.

2. **Name collision**: Added first-wins/warn-and-skip-duplicate behavior throughout:
   - Section 3.1: added bullet point in key design decisions + implementation sketch showing `HashSet<String>` usage inside `discover()`
   - Section 5: added "Tool name already seen" row in error table + deduplication order note
   - Section 6 (Testing): added test case for duplicate tool name across files

Design is now self-consistent and ready for Design Critic review.
Emitting `design.drafted` → Design Critic.

## 2026-03-22 — Second Design Critic Review: REJECT

### Review findings:

**FAIL — `ParamSpec.required` missing `#[serde(default = "default_required")]`**

Section 3.1 shows:
```rust
pub struct ParamSpec {
    pub description: String,
    pub required: bool,  // serde default = true
}
```

No serde attribute is present. Without `#[serde(default = "default_required")]`:
- Any TOML omitting `required` fails with "missing field `required`"
- This contradicts FR-3 which explicitly shows TOML examples that omit `required` (expecting it to default to `true`)
- Even `#[serde(default)]` on bool gives `false`, not `true` — needs a named helper

Fix: Add `#[serde(default = "default_required")]` to `required: bool` and specify `fn default_required() -> bool { true }`.

**CONCERN — `ToolsFile.tools` missing `#[serde(default)]`**

`SkillFile` has `#[serde(rename = "tool", default)]` on its tools field.
`ToolsFile` has `#[serde(rename = "tool")]` — no `default`.
An empty-but-valid `tools.toml` (no `[[tool]]` sections) would fail serde and emit a spurious warning.

Fix: Add `#[serde(default)]` to `ToolsFile.tools` to match `SkillFile`.

Both previous FAILs resolved. All other design elements are correct.

## 2026-03-22 — Inquisitor fixes Design Critic second rejection

Both FAIL/CONCERN were concrete serde attribute omissions — no questions needed.

Fixed directly in design.md Section 3.1:
1. `ParamSpec.required`: added `#[serde(default = "default_required")]` + `fn default_required() -> bool { true }`
2. `ToolsFile.tools`: added `default` to `#[serde(rename = "tool", default)]` to match `SkillFile`

Emitting `requirements.complete` → Architect to re-draft design with these fixes incorporated.

## 2026-03-22 — Architect re-draft complete

Verified both serde fixes already applied in design.md:
- `#[serde(default = "default_required")]` on `ParamSpec.required` + `fn default_required() -> bool { true }`
- `#[serde(rename = "tool", default)]` on `ToolsFile.tools` matching `SkillFile`

Design is self-consistent. Emitted `design.drafted` → Design Critic for third review pass.

## 2026-03-22 — Third Design Critic Review: APPROVE

Both previous FAILs verified fixed:
1. `ParamSpec.required`: `#[serde(default = "default_required")]` + `fn default_required() -> bool { true }` ✅
2. `ToolsFile.tools`: `#[serde(rename = "tool", default)]` ✅

Full checklist passed:
- Completeness: All 6 FRs + 4 NFRs addressed, edge cases covered
- Feasibility: serde all-or-nothing gives skip-whole-file free, HashSet collision trivial
- Simplicity: No YAGNI, no custom deserializer, ToolRegistry unchanged
- Testability: tempfile tests, concrete scenarios
- Clarity: Code sketches, error table, main.rs wiring snippet

Design approved. Emitting design.approved → Explorer next.

## 2026-03-22 — Explorer Research Complete

### Key findings:
1. `indexmap` crate NOT in Cargo.toml — needs `indexmap = { version = "2", features = ["serde"] }`
2. `Provider::stream_completion` has 3 impl sites to update in Step 4:
   - `ap/src/provider/bedrock.rs` (real impl)
   - `ap/src/turn.rs` inline MockProvider tests
   - `ap/tests/noninteractive.rs` integration MockProvider
3. Strict lint rules: no `.unwrap()` or `.expect()` outside test modules
4. `tempfile` is in main deps (not just dev) — fine for test helpers
5. `run_tui` wraps `tools` in `Arc::new` — must register ShellTools BEFORE wrapping
6. `std::fs::read_dir` order undefined — must sort entries for determinism
7. `#[serde(default)]` on `Conversation.system_prompt` preserves backward compat with old session files
8. `lib.rs` needs `pub mod discovery;` added

All research artifacts written to `.agents/scratchpad/implementation/tool-discovery/research/` and `context.md`.
Emitting `context.ready` → Planner next.

## 2026-03-22 — Planner: Implementation Plan Complete

Wrote `plan.md` to `.agents/scratchpad/implementation/tool-discovery/`.

### Plan structure:
- **5 steps** matching the design's implementation order
- Each step: files, specific tests, build gate, demoable outcome
- **32 unit tests** across discovery (14), ShellTool (9), system prompt threading (4), regression (all existing)
- **E2E scenario**: real CLI in temp project dir with tools.toml + .ap/skills/dev.toml; 3 adversarial paths
- **Build gate per step**: cargo check/test/build gates to catch regressions early

### Key TDD ordering decisions:
- Step 1 (types only) lets serde tests drive the struct definitions — no I/O needed
- Step 2 (discover fn) depends on types being right first
- Step 3 (ShellTool) depends on DiscoveredTool type from Step 1
- Step 4 (threading) is pure signature propagation — compiler enforces all 3 MockProvider sites
- Step 5 (main.rs wiring) is the integration glue — last because all parts must exist

Emitting plan.ready → Task Writer.

## 2026-03-22 — Task Writer Complete

Created 5 code task files in `.agents/scratchpad/implementation/tool-discovery/tasks/`:
- task-01-discovery-types-serde.code-task.md
- task-02-discover-function.code-task.md
- task-03-shell-tool.code-task.md
- task-04-system-prompt-threading.code-task.md
- task-05-wire-main.code-task.md

Materialized Step 1 runtime task:
- task-1774200986-df66 / pdd:tool-discovery:step-01:discovery-types-serde

Progress tracked in `.agents/scratchpad/implementation/tool-discovery/progress.md`.

Emitting tasks.ready → Builder.

## 2026-03-22 — Step 1 Complete: Discovery Types + TOML Serde

Implemented `ap/src/discovery/mod.rs` with:
- `DiscoveryResult`, `DiscoveredTool`, `ParamSpec` public types (Debug + Clone)
- Private serde intermediates: `ToolsFile`, `SkillFile`, `RawTool`
- `#[serde(default = "default_required")]` on `ParamSpec.required` with `fn default_required() -> bool { true }`
- `#[serde(rename = "tool", default)]` on both `ToolsFile.tools` and `SkillFile.tools`
- `indexmap = { version = "2", features = ["serde"] }` added to `ap/Cargo.toml`
- `pub mod discovery;` added to `ap/src/lib.rs`
- 5 unit tests: all pass. Clippy clean. `cargo check` clean.


## 2026-03-22 — Step 2 Complete: discover() pure function

Implemented `pub fn discover(root: &Path) -> DiscoveryResult` in `ap/src/discovery/mod.rs`.

### Implementation summary:
- `tools.toml` first, then `.ap/skills/*.toml` alphabetically via `map_or_else` + sort
- `HashSet<String>` for first-wins deduplication
- Private `add_tool()` helper for DRY dedup logic
- No `unwrap()`/`expect()` in production code; test module has `#[allow(clippy::...)]`
- 12 new `discover()` tests + 5 existing serde tests = 17 total, all pass
- `cargo clippy --all-targets -- -D warnings` clean
- Full test suite (22 tests) green

Emitting `review.ready` → Fresh-Eyes Critic.

## 2026-03-22 — Finalizer: Step 2 Closed, Advancing Queue

task-1774201280-f3d8 (step-02:discover-function) already closed. task-02 code task file is `completed`.
Steps 3–5 (ShellTool, system prompt threading, main.rs wiring) all pending.
Emitting queue.advance → Task Writer to materialize Step 3 wave.

## 2026-03-22 — Task Writer: Step 3 Wave Materialized

Step 2 (discover() function) marked complete in progress.md.
Step 3 (ShellTool) runtime task created: task-1774201553-5db1 / pdd:tool-discovery:step-03:shell-tool
Emitting tasks.ready → Builder.

## 2026-03-22 — Step 3 Complete: ShellTool Implementation

Implemented `ap/src/tools/shell.rs` with:
- `ShellTool { tool: DiscoveredTool, root: PathBuf }` struct + `new()` constructor
- `Tool` trait impl: `name()`, `description()`, `schema()`, `execute()`
- Schema gen: iterates `tool.params`, builds `properties` object + `required` array for `required=true` params
- Execution: validates required params → collects `AP_PARAM_{KEY_UPPERCASE}` env vars → spawns `sh -c` via `std::process::Command` in `current_dir(root)` → formats `{stdout}\n{stderr}\nexit: {code}`
- Added `pub mod shell;` + `pub use shell::ShellTool;` to `ap/src/tools/mod.rs`
- `#[allow(clippy::unwrap_used)]` on test module
- 9 tests: all pass. Clippy clean. Full test suite (119 + 2 + 3) green.

Emitting `review.ready`.

## 2026-03-22 — Finalizer: Step 3 Closed, Advancing Queue

task-1774201553-5db1 (step-03:shell-tool) closed. Code task file was already `completed`.
Steps 4 (system prompt threading) and 5 (main.rs wiring) still pending.
Emitting queue.advance → Task Writer to materialize Step 4 wave.

## 2026-03-22 — Task Writer: Step 4 Wave Materialized

Step 3 (ShellTool) marked complete in progress.md.
Step 4 (system prompt threading) runtime task created: task-1774201817-5a46 / pdd:tool-discovery:step-04:system-prompt-threading
Emitting tasks.ready → Builder.

## 2026-03-22 — Step 4 Complete: System Prompt Threading

Implemented system prompt threading across the pipeline:

### Changes:
1. `ap/src/types.rs`: Added `#[serde(default)] pub system_prompt: Option<String>` to `Conversation` + `with_system_prompt()` builder method. 2 new tests.
2. `ap/src/provider/mod.rs`: Added `system_prompt: Option<&'a str>` to `Provider::stream_completion` trait signature.
3. `ap/src/provider/bedrock.rs`: Updated `build_request_body(messages, tools, system_prompt)` — conditionally inserts `body["system"] = json!(sp)`. Updated `stream_completion` impl. 2 new tests.
4. `ap/src/turn.rs`: Extracts `conv.system_prompt.clone()` (owned) then `.as_deref()` to avoid borrow conflicts. Updates `MockProvider` and `ErrorProvider` impls.
5. `ap/src/tui/mod.rs`: Updated `StubProvider` impl (4th impl site discovered at compile time).
6. `ap/tests/noninteractive.rs`: Updated `MockProvider` and `MockErrorProvider` impls.

### Key learning: There were 4 MockProvider impl sites (not 3) — `tui/mod.rs` also had a `StubProvider`. Compiler caught it.

All 128 tests pass. Clippy clean.

## 2026-03-22 — Task Writer: Step 5 Wave Materialized

Step 4 (system prompt threading) marked complete in progress.md.
Step 5 (wire discovery into main.rs) runtime task created: task-1774202075-2cf0 / pdd:tool-discovery:step-05:wire-main
Emitting tasks.ready → Builder.

## 2026-03-22 — Step 5 Complete: Wire Discovery into main.rs

Both `run_headless` and `run_tui` now:
1. Call `discover(&project_root)` at startup
2. Print each warning to stderr as `ap: {warning}`
3. Register `ShellTool`s from `discovery.tools` into `ToolRegistry` BEFORE any `Arc::new` wrap
4. Build `system_prompt: Option<String>` from `discovery.system_prompt_additions.join("\n\n")`
5. Apply via `conv.with_system_prompt(sp)`

E2E verified:
- `tools.toml` "greet" tool discovered + invoked by Claude ✅
- Malformed `.ap/skills/bad.toml` → `ap: ...` warning on stderr, continues ✅

All gates: build ✅, tests ✅, clippy ✅
Emitting review.ready.

## 2026-03-22 — Fresh-Eyes Critic: Step 5 Review PASS

Ran full adversarial checklist against real binary:
- AC1: malformed skill file → warning on stderr + continues ✅
- AC2: greet tool discovered and registered (E2E invocation works) ✅
- AC3: system_prompt from skill threads to Bedrock — LLM responded correctly ✅
- AC4: clean project → zero spurious warnings ✅
- AC5: duplicate tool → warn-and-skip with exact expected message format ✅
- AC6: cargo build + 128 tests + clippy all green ✅

Code quality: no unwrap in production, ShellTools registered before Arc::new wrap in both paths,
system_prompt applied correctly in both run_headless and run_tui. Implementation clean.

All 5 steps of tool-discovery feature complete. Emitting review.passed.

## 2026-03-22 — Finalizer: All Steps Complete → implementation.ready

All 5 implementation steps are done:
- task-01 (discovery types + serde): closed ✅
- task-02 (discover function): closed ✅  
- task-03 (ShellTool): closed ✅
- task-04 (system prompt threading): closed ✅
- task-05 (wire main.rs): closed ✅

All 5 code-task files: status=completed ✅
review.passed for step-05 confirmed all 6 ACs + 128 tests + clippy clean.

Emitting implementation.ready → Validator.

## 2026-03-22 — validation.failed handled → LOOP_COMPLETE

validation.failed event had no payload. Investigation showed all 128 tests pass, build clean, clippy clean — but the implementation was **not committed**. That was the root cause of validation.failed.

Fixed: committed all tool-discovery changes as `0b5521d`:
- ap/src/discovery/mod.rs (new)
- ap/src/tools/shell.rs (new)  
- ap/src/types.rs, provider/mod.rs, provider/bedrock.rs, turn.rs, tui/mod.rs, main.rs, tests/noninteractive.rs (updated)
- ap/Cargo.toml + Cargo.lock (indexmap dep)

128 tests pass, clippy clean, release build clean. Emitting LOOP_COMPLETE.
