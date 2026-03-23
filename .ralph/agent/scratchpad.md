# Session Management UX — Inquisitor Scratchpad

## Iteration 1 — Initial Assessment

### Codebase State
- 216 tests currently passing
- `ap/src/session/mod.rs` — Session struct with `new(id, model)` and `generate(model)`
- `ap/src/session/store.rs` — SessionStore with `save`, `load`, `save_conversation`, `load_conversation`
- `ap/src/main.rs` — Already has `--session <name>` / `-s <name>` flag; TUI + headless paths exist
- Existing store uses flat files: `<base>/<id>.json` (NOT `<base>/<name>/conversation.jsonl`)

### PROMPT.md Requirements vs. Current Implementation

**PROMPT.md specifies:**
1. Auto-name sessions (adjective-noun) on first turn
2. `--prompt "..."` persists to `~/.ap/sessions/<name>/`
3. `--session <name>` / `-s <name>` for resume (already exists as a flag!)
4. `src/session/store.rs` — `save`, `load`, `list`, `generate_name`
5. `--list-sessions` flag
6. Storage: `~/.ap/sessions/<name>/conversation.jsonl` (JSONL per-session dir)

**CURRENT implementation:**
- Session flag exists (`--session`, `-s`)
- Store saves as `<base>/<id>.json` (JSON, not JSONL; flat, not per-directory)
- No `generate_name()` function
- No `list()` function
- No `--list-sessions` flag
- No auto-naming on startup

### Key Gap / Most Critical Unknown

The PROMPT.md specifies storage as `~/.ap/sessions/<name>/conversation.jsonl` (JSONL in a per-session subdir), but the EXISTING store uses `<base>/<id>.json` (JSON in a flat dir). 

This is a **breaking change** to the storage format. There are existing tests relying on the `<id>.json` flat format. Do we:
1. Migrate to JSONL subdir format (breaking existing store tests)
2. Keep both formats (complexity)
3. Just add JSONL as new, keep flat-JSON for backward compat

Also need to understand: does "auto-name" mean ALL sessions get auto-named (no ephemeral runs per Vision), or is it only when `--session` is not specified explicitly?

### Previous Q&A (from idea-honing.md)
- Q1 asked about `created_at` in `Conversation` — still pending answer

### Plan
- Ask one critical question per iteration
- Most critical: The storage format discrepancy could cause test breakage or rework

## Iteration 2 — Answered Q1 & Q2 (Storage Format)

### Decision
- Replace flat `<id>.json` format with JSONL-in-subdir format entirely (option c)
- No `created_at` on `Conversation` — use sidecar `meta.json` per session dir instead
- `store.rs` public API: `save(name, messages)`, `load(name)`, `list()`, `generate_name()`
- Storage: `~/.ap/sessions/<name>/conversation.jsonl` + `~/.ap/sessions/<name>/meta.json`

### Key findings from code research
1. `Session` struct is essentially vestigial — TUI branch discards it immediately (`_session`)
2. Production path uses `save_conversation`/`load_conversation` with `Conversation`
3. 5 store tests will need rewriting, but they're simple
4. 216 tests currently passing

### Next: Inquisitor will decide if more Qs needed or if requirements are complete
