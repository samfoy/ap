Now I have a thorough understanding of the codebase. Let me write the PROMPT.md:Here is the complete `PROMPT.md` written to `~/Projects/ap/ap/PROMPT.md`. Here's a summary of what it covers:

---

### Structure of the PROMPT.md

**Vision** — Zero-friction file editing by default, with opt-in `--safe` approval, dry-run diffs, atomic multi-file apply with rollback, `/undo`, and large-file safety. All consistent with the existing functional-first style.

**Technical Requirements** — Concrete Rust types/signatures for every new construct:
- `PendingWrite`, `EditBatch`, `ApplyResult` in `src/editing/mod.rs`
- `EditingConfig` added to `AppConfig` with `require_approval` and `dry_run`
- Schema field renames (`file`/`old_str`/`new_str`) with legacy fallbacks
- New `PreviewEditTool`
- Four new `TurnEvent` variants (`DryRunDiff`, `BatchApplied`, `BatchRolledBack`, `ApprovalRequired`)
- CLI flags: `--safe`, `--dry-run`, `--undo`

**9 Ordered Steps** — each independently compilable:
1. `EditingConfig` in `AppConfig`
2. Pure data types + `unified_diff` (adds `similar` crate)
3. `apply_batch` + `undo_last` (async I/O, rollback logic, snapshot)
4. Tool schema renames + `preview_edit` tool + 5-tool registry
5. New `TurnEvent` variants + handler updates in TUI and headless routing
6. Wire `EditBatch` into `turn()` — accumulate, apply, dry-run
7. CLI flags `--safe`, `--dry-run`, `--undo`
8. `/undo` slash command in TUI
9. Integration smoke tests in `tests/file_editing.rs`

**17 Acceptance Criteria** covering every spec requirement, ending with the `LOOP_COMPLETE` sentinel.