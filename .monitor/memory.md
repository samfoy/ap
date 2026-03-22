
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

## 2026-03-22 13:50 — AGENTS.md support
Review: **No, it didn't land.** The commits are all backlog housekeeping and three unrelated feature merges (skill-system, tool-discovery, richer-tui) — none of them implement AGENTS.md loading. The infrastructure to inject a system prompt exists (`with_system_prompt`, `discovery.system_prompt_additions`) but there's no code anywhere that reads `~/.ap/AGENTS.md` or `./AGENTS.md` and pipes it through that path.

The gap is concrete: BACKLOG.md marks AGENTS.md support as `[~]` (in progress) but it's phantom progress — it was only *promoted* in priority order, never built. The actual feature (load global + project AGENTS.md, inject into system prompt at startup, hot-reload on next turn) is still entirely missing from `src/`.
Commits:
6a6e0e7 chore(backlog): mark skill-system, tool-discovery, richer-tui as complete
6b50e4f chore(backlog): reorder for bootstrap-first — AGENTS.md and self-hosting promoted to top
13e159b chore(backlog): add Kiro provider with full auth/API implementation notes
90b7054 fchore(monitor): complete {title}
407d022 chore(backlog): add model switching, slack bot, self-hosting, code review items
f6d8d07 feat: merge richer-tui

## 2026-03-22 15:08 — Self-hosting (ap builds ap)
Review: **No, it didn't land.** The "Self-hosting" item is marked `[~]` (in progress) in BACKLOG.md but the two commits in this range are housekeeping only — `acac779` is just `.monitor/memory.md` + log updates, and `f4db103` is a big monitor script hardening (`ap-monitor.py`). Neither touches `ralph.yml` or switches the loop CLI from `pi` to `ap`.

The concrete gap: the three gating prerequisites from the backlog entry — Provider abstraction, AGENTS.md injection, and stable `--print` mode — are themselves incomplete (AGENTS.md was flagged as phantom progress in the previous review entry), so the flip of `cli.backend` to `ap` was never attempted. The monitor got more robust, but ap is still not driving its own loop.
Commits:
5305981 chore: auto-commit before merge (loop primary)
acac779 chore(monitor): complete AGENTS.md support
f4db103 feat(monitor): robust restart — state persistence, stall detection, conflict recovery, push on merge, heartbeat, retry prompts
6a6e0e7 chore(backlog): mark skill-system, tool-discovery, richer-tui as complete
6b50e4f chore(backlog): reorder for bootstrap-first — AGENTS.md and self-hosting promoted to top
13e159b chore(backlog): add Kiro provider with full auth/API implementation notes
