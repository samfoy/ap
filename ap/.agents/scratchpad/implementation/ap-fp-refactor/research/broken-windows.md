# Broken Windows — ap FP Refactor

## [src/tools/mod.rs:67] `with_defaults` uses `register` but doesn't return `Self`
**Type**: inconsistency
**Risk**: Low
**Fix**: The new `.with(tool)` builder pattern (part of refactor) will replace `with_defaults` and use chainable returns. The refactor naturally addresses this.

## [src/app.rs:127-130] `autosave_session` takes `&mut self` but only mutates `session.messages`
**Type**: complexity
**Risk**: Low  
**Fix**: After refactor, `Conversation` owns messages so autosave becomes simpler.

## [src/tui/mod.rs:81] Overly verbose field comment for `agent`
**Type**: docs
**Risk**: Low
**Fix**: "The agent loop, wrapped so it can be shared with spawned tasks" → "Shared agent handle for spawned turn tasks." — worth cleaning up in the refactor.

## [src/hooks/mod.rs:7-17] `HookOutcome::Observed` vs `Proceed` naming is confusing
**Type**: naming
**Risk**: Low  
**Note**: `Proceed` means "allowed to continue" (pre-tool), `Observed` means "saw it but no change" (post-tool/observer). After refactor the bridge adapter will wrap these differently. Not worth changing separately.

## [src/main.rs:37-48] Session loading logic would benefit from a helper function
**Type**: complexity
**Risk**: Low  
**Fix**: Extract `load_or_create_session(id, config) -> Option<Session>` — reduces duplication. Can be done as part of the main.rs refactor step.

## [src/app.rs:46-55] `PendingTool` struct has no doc comment
**Type**: docs
**Risk**: Low
**Fix**: Add `/// Tool call accumulated during provider streaming, not yet executed.` — in the refactor this becomes an inline struct or renamed type.
