
## 2026-03-22 09:49
Completed: FP Refactor
Review: Yes, it landed. The core goal is achieved: `turn()` is a pure function returning `Result<(Conversation, Vec<TurnEvent>)>`, `AgentLoop` and `UiEvent` are deleted, the clippy lint suite is enforcing functional style, and 98 tests pass clean.

The one visible concern is that `turn.rs` still carries significant `mut` throughout its internals — `mut conv`, `mut all_events`, `mut assistant_text`, `mut pending_tools`, etc. — which is expected for accumulator-style async streaming, but it means the "FP boundary" is at the *call site* signature, not the implementation body. That's a reasonable pragmatic trade-off for streaming I/O, but worth noting: the function is pure *in contract* (no side effects, no shared state), not pure *in style*. No structural gaps are visible — TUI decoupling, session persistence, middleware chain, and non-interactive mode all appear covered per the validation commit.
dc0c963 chore: auto-commit before merge (loop primary)
456d4e5 chore: close out ap FP refactor — validation artifacts and task state
7306b72 chore: fix workspace lints — use [lints.clippy] and allow test patterns in test mods
bb160ac chore: add clippy lint suite enforcing functional style
1b940db docs: rewrite README for FP pipeline architecture
ac30210 refactor: delete AgentLoop and legacy UiEvent
6b5551b chore(backlog): add LSP integration as item 11
7e957f1 refactor(turn): return (Conversation, Vec<TurnEvent>) instead of tx side-effect

## 2026-03-22 13:32 — Tool discovery
Review: It landed cleanly. The core goal — `discover()` reading `tools.toml` + `.ap/skills/*.toml`, `ShellTool` implementing the `Tool` trait with `AP_PARAM_*` env injection, and `system_prompt` threading through `Conversation` → `turn()` → `BedrockProvider` — is all present, well-tested (128 passing), and wired into both headless and TUI paths.

One gap worth noting: `ShellTool::execute` uses `std::process::Command` synchronously inside a `BoxFuture` rather than `tokio::process::Command`, so a slow or blocking shell tool will block the async runtime thread. Not a correctness bug today, but it'll cause latency issues under concurrent use.
Commits:
8b33446 chore: auto-commit before merge (loop primary)
0b5521d feat(tool-discovery): add tool discovery, ShellTool, and system prompt threading
ae7c729 chore: init Tool discovery
73d4c96 chore(monitor): start Skill system
4413c22 feat(monitor): parallel worktree-based loops, up to 3 concurrent items
d491188 chore(backlog): all sessions named+persisted from turn 1, no ephemeral runs
