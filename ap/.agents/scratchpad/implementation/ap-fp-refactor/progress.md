# ap FP Refactor — Progress

## Current Step: Step 6

**Status:** Active

## Steps

| Step | Title | Code Task File | Status |
|------|-------|----------------|--------|
| 1 | Core types + ToolRegistry::with() | task-01-core-types-and-registry-builder.code-task.md | completed |
| 2 | Pure turn() function | task-02-pure-turn-function.code-task.md | completed |
| 3 | Middleware chain + shell bridge | task-03-middleware-chain-and-shell-bridge.code-task.md | completed |
| 4 | Session persistence for Conversation | task-04-session-persistence-conversation.code-task.md | completed |
| 5 | main.rs recipe-style + headless | task-05-main-recipe-style-and-headless.code-task.md | completed |
| 6 | turn() sig amendment + TUI decouple | task-06-tui-decouple.code-task.md | active |
| 7 | Delete AgentLoop + UiEvent | task-07-delete-agentloop.code-task.md | pending |
| 8 | README update | task-08-readme-update.code-task.md | pending |
| 9 | Clippy lint suite | task-09-clippy-lint-suite.code-task.md | pending |

## Design Amendments Applied
- **05a (turn() sig):** Merged into Step 6. turn() will return `Result<(Conversation, Vec<TurnEvent>)>`, removing tx sender. Step 6 task file updated accordingly.
- **Step 9 (clippy lints):** New step added after README. Enforce functional style with workspace-level clippy lints.

## Active Wave
- Runtime task for Step 6: to be materialized (queue.advance handling in progress)
