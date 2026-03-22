Here's what the PROMPT.md covers:

**Vision** — frames the "why": projects have specific workflows; tool discovery makes `ap` context-aware so Claude gets named, schematised tools instead of raw bash guessing.

**Technical requirements** with exact Rust types/signatures for:
- `DiscoveryResult` + `DiscoveredTool` (new `src/discovery/mod.rs`)
- `tools.toml` and `.ap/skills/*.toml` file formats with concrete TOML examples
- `ShellTool` implementing the existing `Tool` trait, with `AP_PARAM_*` env var injection
- `Conversation::system_prompt` field + `with_system_prompt()` builder
- Updated `Provider::stream_completion` signature (adds `system_prompt: Option<&'a str>`)
- Threading through `turn()` to the provider

**5 ordered steps**, each independently compilable:
1. `DiscoveredTool` type + TOML serde (pure types, no I/O)
2. `discover()` pure function with `tempfile`-based tests
3. `ShellTool` implementation with execution tests
4. `Conversation`/`Provider` system prompt threading (updates existing signatures)
5. Wiring discovery into `main.rs`

**15 numbered acceptance criteria** — each independently checkable, covering types, purity, parsing, execution, serialisation, provider threading, and safety lints. The final line instructs the loop to emit `LOOP_COMPLETE`.