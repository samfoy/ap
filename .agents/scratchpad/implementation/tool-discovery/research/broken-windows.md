# Broken Windows — tool-discovery

## [ap/src/turn.rs:195] MockProvider `stream_completion` missing `system_prompt`

**Type**: N/A — this will be broken by the Step 4 signature change
**Risk**: Not a broken window — tracked as integration point in context.md
**Note**: This is a REQUIRED change, not optional cleanup.

---

## [ap/tests/noninteractive.rs:37-47] MockProvider `stream_completion` missing `system_prompt`

Same as above — tracked as integration point.

---

## [ap/src/turn.rs:78] `apply_pre_turn` uses `mut conv` parameter

**Type**: naming / style
**Risk**: Low
**Fix**: Not actually a broken window — `mut` on local binding is fine in Rust. No change needed.

---

## [ap/src/tools/mod.rs:88-92] `tool_schemas()` is a redundant alias

**Type**: duplication
**Risk**: Low
**Fix**: `tool_schemas()` is an alias for `all_schemas()` — both exist. Fine to leave as-is (it's used in integration tests and other code may reference it).

---

*No actionable broken windows found in the files touched by this feature. The codebase is clean.*
