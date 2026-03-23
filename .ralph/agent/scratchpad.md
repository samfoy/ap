# Model Switching — Scratchpad

## Iteration 1 — Design Start

### Codebase Analysis

**Config (`ap/src/config.rs`):**
- `ProviderConfig` already has a `model: String` field with default `"us.anthropic.claude-sonnet-4-6"`
- `overlay_from_table` already handles the `model` key from TOML
- Tests already verify `model` config loading
- The config field exists — just the default string may need updating to match the acceptance criteria string

**Provider (`ap/src/provider/`):**
- `Provider` trait: `stream_completion(&self, messages, tools, system_prompt) -> BoxStream`
- `BedrockProvider` stores `model: String` on the struct, clones it per call
- `Arc<dyn Provider>` is used everywhere — no `&mut self` possible without interior mutability

**TUI (`ap/src/tui/mod.rs`):**
- `TuiApp.model_name: String` already exists and is shown in status bar
- `handle_submit()` is where slash commands could be intercepted
- Provider is `Arc<dyn Provider>` shared with spawned tasks

**Main (`ap/src/main.rs`):**
- `Args` struct uses `clap::Parser` — easy to add `--model` flag
- Both `run_headless` and `run_tui` use `config.provider.model`

### Critical Design Decision

The big architectural question: how should `/model` propagate the new model to the provider?

Option A: **Per-call parameter** — add `model: &str` to `stream_completion()` signature. Clean, no interior mutability. `TuiApp.model_name` is the source of truth, passed down each turn.

Option B: **Interior mutability on provider** — `set_model(&self, model: String)` with `Mutex<String>` inside `BedrockProvider`. Works with `Arc<dyn Provider>` but adds shared-state complexity.

The prompt itself mentions both but doesn't specify. This is the key question to ask.

### Status

Answered Q1: Recommended per-call model parameter on `stream_completion()`.

## Iteration 2 — Q1 Answer

### Key discovery: `Conversation.model` already exists

`ap/src/types.rs` has `Conversation { model: String, ... }`. This is the natural carrier.

### Recommended approach: per-call `model: &str` on `stream_completion()`

- Thread `conv.model` through `turn()` → `stream_completion(model, ...)`
- No interior mutability needed
- `/model <name>` sets `self.model_name` (display) and updates `conv.model` (logic)
- Pure pipeline contract preserved
- No race conditions with in-flight tasks

### Changes summary:
1. `Provider` trait: add `model: &str` to `stream_completion()`
2. `BedrockProvider`: use the parameter instead of `self.model`
3. `turn()`: extract `conv.model` and pass to `stream_completion()`
4. `TuiApp.handle_submit()`: intercept `/model <name>`, set model_name + conv.model
5. `main.rs`: add `--model` CLI flag

### Published answer.proposed to hand off to Inquisitor.

## Iteration 3 — Requirements Complete

### Assessment

Q1 (provider mutation strategy) has been answered definitively with per-call model parameter.

All five requirement areas are fully defined:
1. Config: field + default `"us.anthropic.claude-sonnet-4-6-v1:0"` + TOML reading
2. Runtime switching: `/model <name>`, `model_name` + `conv.model` update, confirmation message, inline error
3. Provider: `model: &str` on `stream_completion()`, `Conversation.model` as carrier
4. Status bar: immediate update via `model_name`
5. CLI: `--model` flag overrides config

Error handling: invalid model names surface as AWS errors on next turn — no pre-validation needed.
Scope: session-only, not persisted, no model listing.

Publishing `requirements.complete` → Architect.

## Iteration 5 — Design Critic Review

### Verdict: REJECT with one specific fix needed

### Review Checklist

**Completeness**
- PASS: All 6 requirements from Q&A are addressed
- PASS: Error handling specified for all cases (empty arg, invalid model, in-flight turn)
- **FAIL**: Section 4.4 and 7.1 reference `ChatEntry::System(...)` — this enum variant DOES NOT EXIST.
  - Actual `ChatEntry` variants: `User`, `AssistantStreaming`, `AssistantDone`, `ToolCall`
  - The design references a non-existent variant for the confirmation message and usage message
  - Builder would hit compile errors immediately; fix needed BEFORE handing to Explorer

**Feasibility**
- PASS: Per-call model param on stream_completion() is implementable
- PASS: Conversation.model carrier pattern is solid
- PASS: Provider trait stays object-safe with `model: &'a str`
- PASS: No magic steps; integration points are real

**Simplicity**
- PASS: No speculative features
- PASS: Per-call param is simpler than interior mutability
- PASS: Appendices justify rejected alternatives

**Testability**
- PASS: Concrete test cases specified
- CONCERN: Test for `/model` slash command mentions `ChatEntry::System` - same problem, would fail

**Clarity**
- PASS: Implementation sequence is ordered correctly
- FAIL: `ChatEntry::System` reference is an ambiguous/broken interface spec

### Specific Fix Required

The design must specify how the confirmation message is displayed.
Options:
1. Add a new `ChatEntry::System(String)` variant to the enum (requires documenting this as a new type change)
2. Use `ChatEntry::AssistantDone(vec![ChatBlock::Text(format!("Model switched to: {name}"))])` as a workaround

This is a concrete gap: the design says push `ChatEntry::System(...)` but the type doesn't exist. The Architect must clarify which approach to take and update the design accordingly.

### Emitting design.rejected

## Iteration 9 — Design Critic Full Review (Second Pass)

### Pre-review: Context from codebase verification

1. `ChatEntry` enum: currently `User`, `AssistantStreaming`, `AssistantDone`, `ToolCall` — NO `System` variant. ✅ Design correctly specifies adding it.
2. `Theme.muted` exists in `ap/src/tui/theme.rs`. ✅ Design's render spec is correct.
3. `stream_completion()` currently takes `&messages, &tools, system_prompt` — NO model param. ✅ Design correctly calls out this change.
4. `turn_loop()` currently calls `provider.stream_completion(&messages_snapshot, &tool_schemas, system_prompt)`. ✅ Design's step 4 correctly targets this line.
5. `Conversation.model: String` exists in `ap/src/types.rs`. ✅ Carrier pattern is valid.
6. `conv` in TUI is `Arc<tokio::sync::Mutex<Conversation>>`. The design's handler uses `self.conv.lock().await.model = name`. ✅ Lock pattern is correct.
7. **CRITICAL: `handle_submit` spawns a tokio task** — so the `/model` intercept must happen BEFORE the `tokio::spawn`. The design says "at the top of `handle_submit()`" and returns early — this is correct.
8. `ProviderConfig.model` defaults to `"us.anthropic.claude-sonnet-4-6"` currently; needs updating to `"us.anthropic.claude-sonnet-4-6-v1:0"`. ✅ Design covers this.
9. `Args` struct currently has `prompt`, `session`, `context_limit`, `list_sessions` — no `model`. ✅ Design correctly adds it.
10. `main.rs` uses `config.provider.model.clone()` in multiple places, but the override should happen once before all usages. ✅ Design covers this.

### Review Checklist

**Completeness**
- PASS: All 6 requirements addressed (config default, provider per-call, conversation carrier, /model command, status bar, CLI flag)
- PASS: Error handling specified for all cases (empty arg, invalid model name, in-flight turn race)
- PASS: Edge cases explicitly handled (in-flight turn: "captures prev model at spawn time" — verified correct by reading turn_loop which uses conv.model.clone() at loop start)

**Feasibility**
- PASS: Per-call model param on stream_completion() is implementable — trait stays object-safe with `&'a str`
- PASS: `Conversation.model` as carrier: turn_loop has direct access to conv.model — no magic steps
- PASS: `ChatEntry::System` addition: straightforward enum variant + render match arm
- PASS: handle_submit intercept: returns early before tokio::spawn — no race condition possible
- PASS: main.rs --model override: apply once after AppConfig::load() before provider+conv construction
- PASS: theme.muted exists and is correct

**Simplicity (YAGNI/KISS)**
- PASS: No speculative features
- PASS: Per-call param chosen over interior mutability (justified with clear reasoning in appendix)
- PASS: No unnecessary abstraction layers

**Testability**
- PASS: Concrete test cases specified for each component
- PASS: Unit test for /model updates model_name + conv.model is straightforward
- PASS: Test for ChatEntry::System rendering requires theme — but existing TUI tests already use Theme::default(), so pattern is established
- PASS: Build verification criteria clear (≥204 tests)

**Clarity**
- PASS: Implementation sequence is ordered correctly — trait change first, then all implementors, then callers (steps 2→3→4)
- PASS: Code snippets are accurate and match actual codebase patterns
- PASS: No ambiguous language

### CONCERN (non-blocking): BedrockProvider.model field after change

The design says `self.model` is "retained for documentation/default purposes but is no longer used in `stream_completion()`." This is a dead field. The design acknowledges this in Appendix 9D and explicitly calls it out-of-scope for this feature. The rationale (unchanged constructor signature) is reasonable. This is a PASS — the concern is documented and scoped.

### CONCERN (non-blocking): handle_submit synchronous vs async lock

The design shows `self.conv.lock().await.model = name.to_string()`. Since `handle_submit` is async (it's called with `.await` from the event loop), this is valid. The early-return before `tokio::spawn` means no spawned task can race on this lock. ✅ This is fine.

### VERDICT: APPROVE

All requirements addressed. No fabricated types. Implementation sequence is correct. Error handling is concrete. Build chain is sound. The previous rejection (ChatEntry::System not existing) has been addressed by explicitly adding it as a new variant — this is now documented and specified with render details.

## Iteration 8 — Q2 Resolved, Requirements Complete

### Status

Both blocking questions are answered:
- Q1: Per-call `model: &str` on `stream_completion()` — ✅ decided
- Q2: Add `ChatEntry::System(String)` with `theme.muted` rendering — ✅ decided + design updated

The design.md is fully updated and ready for implementation. Publishing `requirements.complete` → Architect.

---

## Iteration 7 — Q2 Answered: ChatEntry::System Added

### Decision

Added `ChatEntry::System(String)` variant — clean semantic separation, renders in `theme.muted` with `"  ◆ "` prefix. Definitively distinguishable from AI responses.

Updated design.md:
- Section 4.4 renamed to System variant spec + render details
- Section 4.5 now handle_submit with correct `ChatEntry::System` calls
- Section 5 data model diagram includes System variant
- Section 7.1 tests updated to reference System variant
- Section 8 implementation sequence expanded to 9 steps (separate enum change from handler step)

Publishing `answer.proposed` → Inquisitor.

## Iteration 6 — Design Rejected: Addressing ChatEntry::System Gap

### Design Critic Specific Finding

The design in sections 4.4 and 7.1 references `ChatEntry::System(...)` for the confirmation/usage message after `/model` command. This variant does not exist.

Actual variants: `User`, `AssistantStreaming`, `AssistantDone`, `ToolCall`.

The Inquisitor must ask ONE question: which approach should be taken for the feedback message?

Options:
1. Add `ChatEntry::System(String)` — clean, semantic, requires enum change + rendering support
2. Use `ChatEntry::AssistantDone(vec![ChatBlock::Text(...)])` — reuse existing variant, no enum change, but semantically awkward (looks like assistant spoke without a question)

Emitting question.asked → Architect.

## Iteration 4 — Design Drafted

### Summary

Wrote `requirements.md` and `design.md` to `.agents/scratchpad/implementation/model-switching/`.

**Requirements confirmed:**
- Config default: `"us.anthropic.claude-sonnet-4-6-v1:0"` (update existing field)
- Per-call `model: &'a str` param on `Provider::stream_completion()` — chosen over interior mutability
- `Conversation.model` is the carrier through `turn()` → `stream_completion()`
- `/model <name>` in TUI sets `self.model_name` + `conv.model`, shows confirmation inline
- `--model` CLI flag overrides config for headless mode
- Invalid model names surface as AWS errors on next turn (no pre-validation)

**Implementation sequence (for Builder):**
1. config.rs default string
2. Provider trait signature
3. BedrockProvider impl
4. turn.rs model extraction + pass-through
5. tui/mod.rs slash command handler
6. main.rs --model flag
7. Tests update

Publishing design.drafted → Design Critic.

## Iteration 10 — Explorer Phase Complete

### Research findings

**Current test count: 226 passing** (exceeds ≥204 acceptance criteria)

**All `stream_completion` implementations found: 8 total**
- 1 trait definition (`provider/mod.rs:96`)
- 1 real implementation (`provider/bedrock.rs:163`)  
- 2 mock/error providers in `turn.rs` tests (lines 327, 347)
- 2 stub providers in `tui/mod.rs` (lines 368, 1207)
- 2 mock/error providers in `context.rs` tests (lines 298, 317)

**CRITICAL undocumented dependency:** `context.rs:summarise_messages()` also calls `stream_completion()` directly. The design didn't mention this. The fix:
1. Add `model: &str` param to `summarise_messages()`
2. `maybe_compress_context` passes `&conv.model` 
3. Update 2 test providers in context.rs
4. Update 2 test calls to `summarise_messages` to pass model arg

**`/model` intercept position:** Must be BEFORE `chat_history.push(ChatEntry::User(...))` in `handle_submit()` to avoid echoing the slash command as a user message.

**BedrockProvider.model:** Will become dead code after change. May need `#[allow(dead_code)]`.

All research artifacts written to `.agents/scratchpad/implementation/model-switching/research/` and `context.md`.

Publishing `context.ready` → Planner.

---

## Session Management UX — Assessment (New Objective)

### Date: Current iteration

**NOTE:** The scratchpad above contains stale content from the previous "Model Switching" objective.

### Current Objective: Session Management UX

All acceptance criteria already fully implemented:

1. ✅ `ap --prompt "hello"` saves session to `~/.ap/sessions/<name>/`
   - `run_headless()` generates name via `SessionStore::generate_name()`
   - Saves after turn via `store.save(&session_name, &updated_conv)`
   - Prints "Session saved: <name>"

2. ✅ `ap -s swift-river` resumes the session and continues the conversation
   - `session_id` arg loaded via `store.load(&session_name)`
   - History injected via `conv.with_messages(prior_messages)`
   - Works in both headless and TUI mode

3. ✅ `ap --list-sessions` shows all saved sessions with name/date/message count

4. ✅ `cargo build` passes (clean build)

5. ✅ `cargo test` passes — 226 tests (≥204 required)

### Key files:
- `ap/src/session/store.rs` — full SessionStore with save/load/list/generate_name
- `ap/src/session/mod.rs` — Session type, format_unix_as_iso8601
- `ap/src/main.rs` — `run_headless` and `run_tui` both handle session lifecycle

LOOP_COMPLETE — all acceptance criteria met.
