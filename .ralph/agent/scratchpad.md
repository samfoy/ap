# Scratchpad — Model Switching (Inquisitor)

## 2025-01-24 — Iteration 1: design.start

### Context
- spec: model-switching
- PROMPT.md already written (~515 lines) at `/Users/sam.painter/Projects/ap/ap/PROMPT.md`
- requirements.md exists at `.agents/scratchpad/implementation/model-switching/requirements.md`
- idea-honing.md has Q1 + Q2 answered

### Key Discrepancies Found

**PROMPT.md vs requirements.md have two significant gaps:**

1. **`ChatEntry::System` vs `ChatEntry::AssistantDone`**:
   - Q2 answer (→ requirements.md REQ-4) says: add `ChatEntry::System(String)` variant
   - PROMPT.md R4 says: use `ChatEntry::AssistantDone(vec![ChatBlock::Text(...)])` everywhere
   - These are contradictory. If the Architect writes the design to match PROMPT.md, the `ChatEntry::System` variant from requirements.md would be skipped.

2. **`RecentModels` store** — NEW feature in PROMPT.md not in requirements.md:
   - PROMPT.md R2 introduces `~/.ap/models.json` with `RecentModels` struct, `push/load/save` etc.
   - requirements.md REQ-4.5 explicitly says: "Switching is session-scoped only. It is not persisted."
   - PROMPT.md contradicts this with a persistence requirement!

3. **Interception point**:
   - requirements.md: `TuiApp::handle_submit()` intercepts `/model`
   - PROMPT.md: `handle_key_event` intercepts at events layer, returns `Action::ModelSwitch`
   - These are architecturally different (not a problem, but must be reconciled)

### Most Critical Gap to Resolve
The `RecentModels` persistence requirement is the most fundamental contradiction:
- requirements.md explicitly scopes it OUT
- PROMPT.md builds an entire module around it

This needs user clarity before the Architect designs the solution. If persistence is in scope, the Architect needs to design the full `src/models.rs` module. If it's out, the PROMPT.md needs to be revised.

### Q3 to Ask
Ask about the `RecentModels` persistence requirement — it's the most fundamental scope expansion not addressed in requirements.

### Task ID
task-1774279300-01a8 (key: pdd:model-switching:requirements)
