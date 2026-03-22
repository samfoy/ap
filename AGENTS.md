# AGENTS.md — ap project

## Coding Style

This project follows **functional-first** Rust. See the global AGENTS.md for the full philosophy. Key rules for this codebase:

- `turn()` is a pure function — no side effects except the mpsc sender
- `Conversation` is immutable — each turn returns a new one
- Middleware is a chain of `Fn` closures, not trait objects with state
- Iterator chains over imperative loops
- `mut` is a red flag — justify it if you use it

## Architecture

- `src/types.rs` — core data types (`Conversation`, `TurnEvent`, `ToolCall`, `Middleware`)
- `src/turn.rs` — pure `turn()` pipeline
- `src/middleware.rs` — `Middleware` chain + shell hook bridge
- `src/provider/` — `Provider` trait + `BedrockProvider`
- `src/tools/` — `Tool` trait + 4 built-ins (read, write, edit, bash)
- `src/tui/` — ratatui UI, wired to `TurnEvent`
- `src/session/` — `Conversation` persistence

## What "hackable" means here

No extension system. If you want new behavior: edit the source. `main.rs` is the composition root — wire new tools, middleware, or providers there.
