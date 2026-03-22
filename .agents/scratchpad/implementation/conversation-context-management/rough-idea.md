# Rough Idea: Conversation Context Management

Source: `/Users/sam.painter/Projects/ap/PROMPT.md` (also at `PROMPT.md` in this worktree)

## Core Objective

Prevent context window exhaustion in long coding sessions by:

1. Estimating token usage before every LLM call
2. Auto-summarising old messages when estimated tokens cross a configurable threshold
3. Surfacing context usage in the TUI status bar
4. Exposing `--context-limit N` CLI flag and `[context] limit = N` TOML config

## Key Constraints (from AGENTS.md + spec)

- `maybe_compress_context` is async standalone fn, NOT middleware (middleware is sync)
- Functional-first: `Conversation` is immutable, no `mut` globals
- Every step must compile and pass `cargo test` before the next
- 7 ordered implementation steps defined in PROMPT.md
- 12 acceptance criteria defined in PROMPT.md

## Implementation Steps (from PROMPT.md)

1. `src/context.rs` — pure token estimation + `find_summary_split`
2. `ContextConfig` in `AppConfig`
3. `TurnEvent::ContextSummarized` variant + `--context-limit` CLI flag
4. TUI: `last_input_tokens` field + status bar updates
5. `src/context.rs` async summarisation functions
6. Wire `maybe_compress_context` in headless path
7. Wire `maybe_compress_context` in TUI path + `context_limit` field
