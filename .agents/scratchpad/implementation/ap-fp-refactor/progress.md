# ap FP Refactor — Progress

## Wave Schedule

| Step | Title | Status |
|------|-------|--------|
| 01 | Core types + ToolRegistry::with() builder | completed |
| 02 | Pure turn() function | completed |
| 03 | Middleware chain + shell hook bridge | completed |
| 04 | Session persistence for Conversation | completed |
| 05 | main.rs recipe-style + headless mode | completed |
| 06 | TUI decoupling | completed |
| 07 | Delete AgentLoop | active |
| 08 | README update | pending |
| 09 | Clippy lint suite | pending |

## Active Wave

Step 07 — Delete AgentLoop (task-1774196788-76c8, key: pdd:ap-fp-refactor:step-07:delete-agentloop)

## Completed

- Step 01: commit 34df8f4 — src/types.rs + ToolRegistry::with() builder; 88 tests pass
- Step 02: commit f717304 — src/turn.rs pure turn() pipeline; 95 tests pass
- Step 03: commit 4dfc273 — src/middleware.rs + shell hook bridge; 101 tests pass
- Step 04: commit 392ebda — SessionStore::save_conversation/load_conversation; 105 tests pass
- Step 05: commit 071af57 — main.rs headless + noninteractive tests using turn()/TurnEvent; 105 tests pass
- Step 06: commit 7e957f1 — turn() returns (Conversation, Vec<TurnEvent>), TuiApp decoupled from AgentLoop; 107 tests pass

## design.amendment (pending, must precede Step 06)

Change turn() signature from:
  `turn(conv, provider, tools, middleware, tx: &mpsc::Sender<TurnEvent>) -> Result<Conversation>`
To:
  `turn(conv, provider, tools, middleware) -> Result<(Conversation, Vec<TurnEvent>)>`

Callers route events themselves. Apply this change BEFORE or AS PART OF Step 06 (TUI decouple).
Affects: turn.rs, main.rs (headless loop), tests/noninteractive.rs, turn tests.
