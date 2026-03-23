# PROMPT.md — Robust File Editing

## Vision

`ap` currently has basic `edit` and `write` tools that work in isolation but
lack the safety and ergonomics expected of a production coding agent.  This
feature hardens file editing end-to-end:

- **Zero friction by default** — edits apply immediately, no confirmation
  dialogs. Trust the user. A `--safe` flag adds approval prompts for those who
  want them.
- **Dry-run / diff preview** — `--dry-run` shows a unified diff to stdout
  without writing anything. Claude can call a `preview_edit` tool to display a
  diff before applying.
- **Atomic multi-file apply** — all writes/edits requested during a turn are
  buffered, then written together. If any write fails, every already-written
  file is rolled back to its pre-edit state.
- **Undo** — `/undo` in the TUI (or `--undo` flag headless) reverts the last
  batch of edits by restoring pre-edit snapshots from `~/.ap/undo/`.
- **Large file safety** — files >1000 lines warn when `old_str` spans >50
  lines; the tool validates that `old_str` is actually present before touching
  the file. The `write` tool stores a snapshot before overwriting.
- **Config** — a new `[editing]` TOML section controls behaviour.

The implementation must stay consistent with the project's functional-first
style: pure functions, immutable data, iterator chains, no hidden mutation.
Every step must compile cleanly and pass `cargo test` before moving to the
next.

---

## Technical Requirements

### New types (add to `src/editing/mod.rs` unless noted)

```rust
// src/editing/mod.rs

/// A single pending write, buffered before atomic apply.
#[derive(Debug, Clone)]
pub struct PendingWrite {
    /// Absolute path of the file to write.
    pub path: PathBuf,
    /// New content to write.
    pub new_content: String,
    /// Content that was on disk before any edit in this batch
    /// (None if the file did not exist).
    pub original_content: Option<String>,
}

/// Outcome of `apply_batch`.
#[derive(Debug)]
pub struct ApplyResult {
    /// Files successfully written (absolute paths).
    pub written: Vec<PathBuf>,
    /// The first error that caused a rollback, if any.
    pub error: Option<(PathBuf, std::io::Error)>,
}

/// A batch of file operations accumulated during one turn.
/// Pure value — callers clone/replace, never mutate in place.
#[derive(Debug, Clone, Default)]
pub struct EditBatch {
    pub writes: Vec<PendingWrite>,
}

impl EditBatch {
    pub fn new() -> Self { Self::default() }

    /// Add a write to the batch.  If `path` already appears, replace it
    /// (last-write-wins within one turn).
    pub fn add_write(self, pending: PendingWrite) -> Self;

    /// Return all pending paths (for display / preview).
    pub fn paths(&self) -> Vec<&PathBuf>;
}

/// Compute a unified diff string between two texts.
/// Returns an empty string when old == new.
pub fn unified_diff(old: &str, new: &str, path: &str) -> String;

/// Apply `batch` atomically: write all files, rolling back on the first
/// failure.  Saves pre-edit snapshots into `undo_dir` before writing.
/// Pure-ish: all I/O is isolated here, callers stay pure.
pub async fn apply_batch(
    batch: &EditBatch,
    undo_dir: &Path,
) -> ApplyResult;

/// Restore the last snapshot from `undo_dir`, returning the list of
/// files restored or an error.
pub async fn undo_last(undo_dir: &Path) -> anyhow::Result<Vec<PathBuf>>;
```

```rust
// src/config.rs  — add to AppConfig / overlay_from_table

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditingConfig {
    /// When true, every edit/write requires user confirmation before applying.
    pub require_approval: bool,
    /// When true, show a unified diff but do not write anything.
    pub dry_run: bool,
}

impl Default for EditingConfig {
    fn default() -> Self {
        Self { require_approval: false, dry_run: false }
    }
}
// AppConfig gains:  pub editing: EditingConfig
```

```rust
// src/tools/edit.rs  — updated schema and parameter names

// Schema field names change to match Claude Code / pi convention:
//   "file"    (was "path")
//   "old_str" (was "old_text")
//   "new_str" (was "new_text")
// The execute() function must accept BOTH old and new names for backward
// compat during the transition (check "file" first, fall back to "path";
// "old_str" first, fall back to "old_text"; etc.)
```

```rust
// src/tools/write.rs  — updated schema field name

// Schema field "file" (was "path").  Same fallback rule as edit.
```

```rust
// src/tools/preview_edit.rs  — new tool

pub struct PreviewEditTool;
// schema: { "file": string, "old_str": string, "new_str": string }
// execute(): computes unified_diff, returns it as ToolResult::ok(diff_text)
// Does NOT write anything — pure read + diff.
```

```rust
// src/turn.rs  — accumulate EditBatch during tool execution

// After all tool calls in a turn complete, if EditBatch is non-empty:
//   - if dry_run  → emit TurnEvent::DryRunDiff(String) for each pending write,
//                    do not apply, clear batch.
//   - if require_approval → emit TurnEvent::ApprovalRequired(EditBatch),
//                    suspend until user responds via new TurnCommand::Approve /
//                    TurnCommand::Reject channel (see Step 7).
//   - otherwise   → call apply_batch(), emit TurnEvent::BatchApplied or
//                    TurnEvent::BatchRolledBack.
```

```rust
// src/types.rs  — new TurnEvent variants

pub enum TurnEvent {
    // …existing variants…

    /// A unified diff to display (dry-run mode — nothing was written).
    DryRunDiff { path: String, diff: String },

    /// All edits in the batch were applied successfully.
    BatchApplied { paths: Vec<String> },

    /// A write failed; all already-written files were rolled back.
    BatchRolledBack { failed_path: String, reason: String },

    /// Edits are pending approval (--safe mode).
    ApprovalRequired { paths: Vec<String> },
}
```

```rust
// src/main.rs  — CLI flags

#[derive(Parser)]
struct Args {
    // …existing flags…

    /// Require approval before applying any file edits.
    #[arg(long)]
    safe: bool,

    /// Show a unified diff of all edits but do not write anything.
    #[arg(long)]
    dry_run: bool,

    /// Revert the last batch of file edits.
    #[arg(long)]
    undo: bool,
}
```

```rust
// src/tui/events.rs  — /undo slash command

// In handle_submit(), check for "/undo" before dispatching to turn().
// Call undo_last(undo_dir) and push a ChatEntry reporting what was restored.
```

---

## Ordered Implementation Steps

Each step must leave the project in a **compilable, all-tests-passing** state.

---

### Step 1 — `EditingConfig` in `AppConfig`

**Files:** `src/config.rs`

1. Add `EditingConfig` struct with `require_approval: bool = false` and
   `dry_run: bool = false`, both deriving `Serialize + Deserialize + Default`.
2. Add `pub editing: EditingConfig` field to `AppConfig` (with `#[serde(default)]`).
3. Extend `overlay_from_table` to handle `[editing]` section (same key-present
   guard pattern used for every other section).
4. Add unit tests in `config.rs`:
   - `editing_config_defaults()` — `require_approval` false, `dry_run` false.
   - `editing_config_toml_require_approval()` — parses `require_approval = true`.
   - `editing_config_toml_dry_run()` — parses `dry_run = true`.
   - `editing_config_missing_keys_preserve_defaults()` — empty `[editing]`
     table leaves both fields at default.
   - `editing_config_not_serialized_to_sessions()` — existing conversation JSON
     without `"editing"` key deserializes without error.

**Acceptance criteria:** `cargo test config` passes; no other tests regress.

---

### Step 2 — `src/editing/mod.rs` — pure data types + `unified_diff`

**Files:** `src/editing/mod.rs` (new), `src/lib.rs`

1. Create `src/editing/mod.rs`.  Add `pub mod editing;` to `src/lib.rs`.
2. Implement `PendingWrite`, `ApplyResult`, `EditBatch` exactly as specified.
3. Implement `EditBatch::add_write(self, pending: PendingWrite) -> Self`
   (consuming, replaces existing entry for the same path).
4. Implement `EditBatch::paths(&self) -> Vec<&PathBuf>`.
5. Implement `unified_diff(old: &str, new: &str, path: &str) -> String`
   — use the `similar` crate (add `similar = "2"` to `Cargo.toml`).
   Format: standard unified diff header `--- a/<path>` / `+++ b/<path>`,
   `@@ … @@` hunks.  Return `""` when `old == new`.
6. Unit tests in `src/editing/mod.rs`:
   - `edit_batch_add_write_appends()` — new path appended.
   - `edit_batch_add_write_replaces_existing()` — same path replaces old entry.
   - `edit_batch_paths_returns_all()` — `paths()` returns one entry per write.
   - `unified_diff_empty_when_equal()` — same content → `""`.
   - `unified_diff_shows_change()` — changed line appears in diff output.
   - `unified_diff_added_file()` — empty old, non-empty new.
   - `unified_diff_deleted_file()` — non-empty old, empty new.

**Acceptance criteria:** `cargo test editing` passes.

---

### Step 3 — `apply_batch` + `undo_last`

**Files:** `src/editing/mod.rs`

1. Implement `apply_batch(batch: &EditBatch, undo_dir: &Path) -> ApplyResult`
   (async):
   - Create `undo_dir` if absent.
   - Before writing any file: serialize the entire batch's original-content
     map to `<undo_dir>/last.json`  
     (`{"<abs-path>": "<original_content_or_null>", …}`).
   - Iterate `batch.writes` in order; for each:
     - Create parent directories as needed.
     - Write `new_content` to the path.
     - On `Err`: roll back every file already written (restore from
       `original_content` in the `PendingWrite`; delete if `None`), then
       return `ApplyResult { written: already_written, error: Some((path, e)) }`.
   - On full success: return `ApplyResult { written: all_paths, error: None }`.
2. Implement `undo_last(undo_dir: &Path) -> anyhow::Result<Vec<PathBuf>>`:
   - Read `<undo_dir>/last.json`.
   - For each entry: if value is a string, write it back; if `null`, delete
     the file (if it exists).
   - Remove `last.json` after successful restore.
   - Return the list of paths restored.
3. Unit tests:
   - `apply_batch_writes_all_files()` — two writes both appear on disk.
   - `apply_batch_creates_parent_dirs()` — nested path created.
   - `apply_batch_saves_undo_snapshot()` — `last.json` created in `undo_dir`.
   - `apply_batch_rollback_on_write_failure()` — simulate failure for the
     second file by using a read-only directory; first file is rolled back.
   - `undo_last_restores_files()` — after `apply_batch`, `undo_last` restores.
   - `undo_last_deletes_new_file()` — original_content `None` → file deleted.
   - `undo_last_error_when_no_snapshot()` — `undo_dir` empty → `Err`.

**Acceptance criteria:** `cargo test editing` passes.

---

### Step 4 — Update `edit` and `write` tool schemas + add `preview_edit`

**Files:** `src/tools/edit.rs`, `src/tools/write.rs`,
`src/tools/preview_edit.rs` (new), `src/tools/mod.rs`

1. **`edit` tool:**
   - Change schema fields to `"file"`, `"old_str"`, `"new_str"`.
   - In `execute()`, read params with fallback:
     ```
     file   = params["file"] ?? params["path"]
     old_str = params["old_str"] ?? params["old_text"]
     new_str = params["new_str"] ?? params["new_text"]
     ```
   - Existing behaviour (unique-match replace) unchanged.
   - **Large-file warning:** if the file has >1000 lines AND `old_str`
     contains >50 newlines, prepend  
     `"[large-file warning: old_str spans >50 lines] "` to the success
     message.
2. **`write` tool:**
   - Change schema field to `"file"` (with `"path"` fallback).
3. **`preview_edit` tool (new):**
   - Schema: `{ "file": string, "old_str": string, "new_str": string }`.
   - `execute()`: read the file, substitute `old_str → new_str` (same unique-
     match logic as `edit`), compute `unified_diff`, return the diff as
     `ToolResult::ok(diff)`.  Write nothing.
   - If `old_str` not found or matches multiple times, return `ToolResult::err`.
4. Register `PreviewEditTool` in `ToolRegistry::with_defaults()` (5 tools now).
5. Update `pub use` in `src/tools/mod.rs`.
6. Tests:
   - `edit_schema_uses_file_key()` — schema has `"file"` not `"path"`.
   - `edit_accepts_legacy_path_key()` — `{"path":…, "old_text":…, "new_text":…}`
     still works.
   - `edit_large_file_warning()` — file with 1001 lines, `old_str` spanning
     51 lines → success message contains `"large-file warning"`.
   - `write_schema_uses_file_key()` — schema has `"file"`.
   - `write_accepts_legacy_path_key()` — `{"path":…}` still works.
   - `preview_edit_returns_diff()` — diff text contains `"-old"` and `"+new"`.
   - `preview_edit_does_not_write()` — file on disk is unchanged after call.
   - `preview_edit_errors_on_missing_old_str()` — error when `old_str` absent.
   - `registry_with_defaults_has_five_schemas()` — replaces old 4-schema test.

**Acceptance criteria:** `cargo test tools` passes; all old tool tests updated
or passing.

---

### Step 5 — New `TurnEvent` variants

**Files:** `src/types.rs`

1. Add to `TurnEvent`:
   ```rust
   DryRunDiff { path: String, diff: String },
   BatchApplied { paths: Vec<String> },
   BatchRolledBack { failed_path: String, reason: String },
   ApprovalRequired { paths: Vec<String> },
   ```
2. All new variants must be `Clone`.
3. Tests in `src/types.rs`:
   - `turn_event_dry_run_diff_clonable()`
   - `turn_event_batch_applied_clonable()`
   - `turn_event_batch_rolled_back_clonable()`
   - `turn_event_approval_required_clonable()`
4. Update `route_headless_events` in `src/main.rs` to handle the four new
   variants (print to stdout/stderr as appropriate; `BatchRolledBack` sets
   `exit_code = 1`).
5. Update `TuiApp::handle_ui_event` in `src/tui/mod.rs` to handle the four
   new variants:
   - `DryRunDiff` → push `ChatEntry::AssistantDone` showing the diff path +
     a code block with the diff text.
   - `BatchApplied` → push a notice like `"[Edited: a.rs, b.rs]"`.
   - `BatchRolledBack` → push an error notice.
   - `ApprovalRequired` → push a notice listing pending paths.
6. Tests in `src/tui/mod.rs` for each new handler (headless app only).

**Acceptance criteria:** `cargo test types` and `cargo test tui` pass.

---

### Step 6 — Wire `EditBatch` into `turn()`

**Files:** `src/turn.rs`

`turn()` signature stays the same. Internally:

1. Add a local `edit_batch: EditBatch` that accumulates writes produced by
   `edit` and `write` tool calls.
   - After each successful `edit` tool execution, read the new file content
     from disk and add a `PendingWrite` with `original_content` = content
     before the edit.
   - After each successful `write` tool execution, add a `PendingWrite` with
     `original_content` = file content before the write (or `None` if new).
   - `preview_edit` and all other tools are not added to the batch.
2. After all tool calls in a round complete (before looping back to the LLM):
   - If `edit_batch` is non-empty:
     - If `conv.config.editing.dry_run`: compute diffs, emit
       `TurnEvent::DryRunDiff` for each pending write, **do not apply**,
       clear batch.
     - Otherwise: call `apply_batch(&batch, &undo_dir)`.
       - On success: emit `TurnEvent::BatchApplied { paths }`.
       - On rollback: emit `TurnEvent::BatchRolledBack { failed_path, reason }`,
         also emit `TurnEvent::Error(…)` so the caller can short-circuit.
   - `require_approval` path (Step 7) is a no-op stub that falls through to
     normal apply for now.
3. The `undo_dir` is `dirs::home_dir()/.ap/undo/` (create if absent).
4. Tests in `src/turn.rs` (using `MockProvider` pattern already established):
   - `turn_edit_batch_applied_after_edit_tool()` — mock provider emits a
     `write` tool call; `TurnEvent::BatchApplied` present in events.
   - `turn_dry_run_emits_diff_no_write()` — `conv.config.editing.dry_run =
     true`; `TurnEvent::DryRunDiff` emitted; file on disk unchanged.
   - `turn_batch_rolled_back_on_write_failure()` — simulate failure (read-only
     target dir); `TurnEvent::BatchRolledBack` + `TurnEvent::Error` emitted.

**Acceptance criteria:** `cargo test turn` passes.

---

### Step 7 — CLI flags: `--safe`, `--dry-run`, `--undo`

**Files:** `src/main.rs`

1. Add `--safe` flag → sets `config.editing.require_approval = true`.
2. Add `--dry-run` flag → sets `config.editing.dry_run = true`.
3. Add `--undo` flag → runs `undo_last(undo_dir)` and exits before any turn:
   - On success: print `"reverted: <path1>, <path2>, …"` to stdout; exit 0.
   - On error: print error to stderr; exit 1.
4. `require_approval = true` path in `turn()`: instead of calling
   `apply_batch`, emit `TurnEvent::ApprovalRequired { paths }` and
   **do not write** (stub — full interactive approval is a future feature;
   this satisfies the spec's `--safe` flag by surfacing the pending writes
   without applying them).
5. Tests in `src/main.rs`:
   - `dry_run_flag_sets_config()` — parse `["--dry-run"]`; confirm
     `config.editing.dry_run == true`.
   - `safe_flag_sets_config()` — parse `["--safe"]`; confirm
     `config.editing.require_approval == true`.
   - `undo_flag_parsed()` — parse `["--undo"]`; confirm flag is present.

**Acceptance criteria:** `cargo test` (full suite) passes; `cargo build`
produces a binary with `--help` listing all three new flags.

---

### Step 8 — `/undo` in the TUI

**Files:** `src/tui/mod.rs`

1. In `handle_submit()`, before dispatching to the turn task, check:
   ```rust
   if trimmed == "/undo" { … }
   ```
2. Call `ap::editing::undo_last(&undo_dir).await` (spawn a task or use
   `tokio::task::spawn_blocking` as appropriate).
3. On success: push a `ChatEntry::AssistantDone` with text  
   `"[Reverted: <path1>, <path2>, …]"`.
4. On error: push a `ChatEntry::AssistantDone` with  
   `"[Undo failed: <error>]"`.
5. In both cases, do **not** call `turn()`.
6. Tests in `src/tui/mod.rs` (headless, using a temp undo dir):
   - `undo_command_does_not_call_turn()` — after `/undo` with an empty undo
     dir, `is_waiting` stays `false`.
   - This test may use a test-only helper that injects a custom undo dir;
     if that adds complexity, a smoke-test asserting the slash-command branch
     is taken (no turn started) is sufficient.

**Acceptance criteria:** `cargo test tui` passes.

---

### Step 9 — Integration smoke test

**Files:** `tests/file_editing.rs` (new)

Write an integration test (no network, uses `MockProvider`) that exercises the
full pipeline end-to-end:

```
1. Create a temp file with known content.
2. Build a Conversation with editing.dry_run = false.
3. Run turn() with a MockProvider that emits a `write` tool call.
4. Assert TurnEvent::BatchApplied is in the events.
5. Assert the file on disk has the new content.
6. Run undo_last() on the undo dir.
7. Assert the file is back to original content.
```

A second test covers dry-run:
```
1. Same setup, but editing.dry_run = true.
2. Assert TurnEvent::DryRunDiff is emitted.
3. Assert file on disk is UNCHANGED.
```

**Acceptance criteria:** `cargo test --test file_editing` passes.

---

## Acceptance Criteria (full feature)

- [ ] **AC-1** `[editing]` section in TOML config parsed correctly; defaults
  are `require_approval = false`, `dry_run = false`.
- [ ] **AC-2** `EditBatch`, `PendingWrite`, `ApplyResult` compile and have
  full unit-test coverage.
- [ ] **AC-3** `unified_diff` returns `""` for identical content and a valid
  unified diff for changes.
- [ ] **AC-4** `apply_batch` writes all files atomically; on first failure
  rolls back already-written files; saves `last.json` snapshot.
- [ ] **AC-5** `undo_last` restores all files from `last.json`; deletes
  newly-created files (original `null`); errors gracefully when no snapshot.
- [ ] **AC-6** `edit` tool schema uses `"file"` / `"old_str"` / `"new_str"`;
  still accepts legacy `"path"` / `"old_text"` / `"new_text"`.
- [ ] **AC-7** `write` tool schema uses `"file"`; still accepts legacy `"path"`.
- [ ] **AC-8** `preview_edit` tool returns unified diff, writes nothing.
- [ ] **AC-9** `TurnEvent::DryRunDiff`, `BatchApplied`, `BatchRolledBack`,
  `ApprovalRequired` all exist, are `Clone`, are handled in both headless
  output routing and TUI event handler.
- [ ] **AC-10** `turn()` accumulates edits into `EditBatch` and calls
  `apply_batch` (or emits `DryRunDiff`) after each tool-execution round.
- [ ] **AC-11** `--dry-run` CLI flag sets `editing.dry_run = true`; no files
  written; `DryRunDiff` events emitted.
- [ ] **AC-12** `--safe` CLI flag sets `editing.require_approval = true`;
  `ApprovalRequired` event emitted; no files written.
- [ ] **AC-13** `--undo` CLI flag restores last batch and exits.
- [ ] **AC-14** `/undo` TUI slash command restores last batch and reports
  result in the chat pane.
- [ ] **AC-15** Integration test in `tests/file_editing.rs` passes for both
  normal apply and dry-run paths.
- [ ] **AC-16** `cargo build` produces a clean binary; `cargo test` passes
  with zero failures; `cargo clippy -- -D warnings` produces no warnings.
- [ ] **AC-17** `ToolRegistry::with_defaults()` now registers 5 tools
  (`read`, `write`, `edit`, `bash`, `preview_edit`).

---

## Dependencies to add to `Cargo.toml`

```toml
# Unified diff generation
similar = "2"
```

---

## File Map

```
src/
  config.rs           ← add EditingConfig, overlay_from_table extension
  editing/
    mod.rs            ← NEW: PendingWrite, EditBatch, ApplyResult,
                              unified_diff, apply_batch, undo_last
  lib.rs              ← add: pub mod editing;
  tools/
    edit.rs           ← schema rename + legacy fallback + large-file warning
    write.rs          ← schema rename + legacy fallback
    preview_edit.rs   ← NEW
    mod.rs            ← register PreviewEditTool, update with_defaults()
  turn.rs             ← accumulate EditBatch, wire apply_batch / dry-run
  types.rs            ← add 4 new TurnEvent variants
  tui/
    mod.rs            ← handle new TurnEvent variants, /undo command
  main.rs             ← --safe, --dry-run, --undo flags
tests/
  file_editing.rs     ← NEW integration tests
```

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project
builds clean.
