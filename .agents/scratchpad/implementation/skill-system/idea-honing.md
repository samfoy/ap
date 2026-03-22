# Idea Honing: Skill System

## Q&A Session

---

### Q1 — Inquisitor

The spec defines `system_prompt: Option<String>` on `Conversation` with `#[serde(default)]`,
meaning it serialises to disk with the session. The `skill_injection_middleware` is specified
to *append* to any existing `system_prompt` on the conversation.

**Critical gap**: When a conversation is saved after a turn (with `system_prompt` set to the
injected skills block) and then loaded in a future session, the middleware will append the
freshly-scored skills *on top of* the old persisted `system_prompt`. Over multiple sessions,
this accumulates stale injected content.

**Question**: Should `system_prompt` be a *transient, runtime-only* field (excluded from
session serialisation, e.g. `#[serde(skip)]`), so each session starts clean and the
middleware always replaces rather than appends? Or should it persist across sessions — and
if so, should the middleware replace rather than append?

### A1 — Architect

**Decision: `#[serde(skip)]` — transient field, middleware sets (replaces, never appends).**

**Rationale:**

`system_prompt` is *derived* content — it is computed at runtime from the skills directory
and the current conversation messages. It is categorically different from the persisted user
data in a session (`messages`, `model`, `id`, `config`).

Three reasons `#[serde(skip)]` is the right choice:

1. **Skills files change between sessions.** The user may edit, add, or remove `.md` skill
   files. Persisting a stale snapshot means sessions would silently use outdated content even
   after the user updated their skill library.

2. **TF-IDF scores are conversation-relative.** The correct skills for a session depend on
   what the user has actually said so far. Persisting a prior scoring locks in relevance
   judgements from a previous context.

3. **Derivable data should not be persisted.** The existing codebase already demonstrates
   this principle: `config` uses `#[serde(default)]` precisely because it is ambient
   configuration, not conversation state. `system_prompt` is one step further — it is
   *computed* from config + runtime context, so it should never be written to disk at all.

**Consequence for middleware behaviour:** Since the field starts as `None` on every load,
`skill_injection_middleware` simply *sets* `system_prompt` to the freshly-scored block — no
append logic needed. This is simpler, testable in isolation, and immune to accumulation.

**Impact on the spec:**
- Step 1: `system_prompt: Option<String>` with `#[serde(skip)]` (not `#[serde(default)]`)
- Step 5: middleware sets `conv.system_prompt = Some(block)` — not append

All other steps are unaffected.

---

### Q2 — Inquisitor (triggered by design.rejected: FAIL-1 + FAIL-2)

**Context:** The design document was rejected for two contradictions. FAIL-2 (empty
`select_skills` result unconditionally calls `skills_to_system_prompt`) has a clear answer:
if no skills match, the middleware should return `None` (leave the conversation unchanged) —
there is no sensible alternative to injecting an empty header. This should become an
explicit guard in FR-7.

**FAIL-1 requires user input.** The design document describes three incompatible wiring
patterns for `skill_injection_middleware`:
- Appendix C: returns a raw closure, caller uses `mw = mw.pre_turn(skill_injection_middleware(...))`
- FR-7 + D-01: returns a full `Middleware` struct, then `middleware.pre_turn.extend(skill_mw.pre_turn)`
- Section 4.8: the `.remove(0)` approach, immediately abandoned

The existing codebase has one precedent: `shell_hook_bridge(&config.hooks) -> Middleware`.
In `main.rs` today, this is the *only* middleware — there's no multi-middleware merge yet.

**Question**: Should `skill_injection_middleware` follow the same pattern as
`shell_hook_bridge` and return a full `Middleware` struct, with a new `Middleware::merge(other: Middleware) -> Self`
(or `.extend(other)`) builder method added to support composing the two in `main.rs`? Or
should it return a closure directly so the caller simply writes:
```rust
let middleware = shell_hook_bridge(&config.hooks)
    .pre_turn(skill_injection_middleware(...));
```

### A2 — Architect

**Decision: `skill_injection_middleware` returns a closure; caller composes via `.pre_turn(...)`.**

**Rationale:**

The existing `Middleware` builder API already has the perfect composition primitive:

```rust
pub fn pre_turn(
    mut self,
    f: impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static,
) -> Self
```

Returning a closure means the caller writes exactly:

```rust
let middleware = shell_hook_bridge(&config.hooks)
    .pre_turn(skill_injection_middleware(&config.skills));
```

This is:
1. **Zero new API surface** — no `Middleware::merge` method to design, implement, and test.
2. **Consistent with the consuming builder pattern** — the same style as every other closure
   registration in the codebase (`Middleware::new().pre_tool(...).pre_turn(...)`).
3. **Structurally simpler** — `skill_injection_middleware` is a `pre_turn` concern only.
   Returning a full `Middleware` struct would be misleading (it would have empty `pre_tool`,
   `post_tool`, `post_turn` vecs) and require callers to know to extract just the
   `pre_turn` vec — or use a merge operation.
4. **No merge footgun** — A `merge` method would be correct only if field order is preserved
   and all four vec fields are appended. That's non-trivial to reason about for callers; a
   raw closure passed to the known-slot `.pre_turn()` is unambiguous.

**Canonical `skill_injection_middleware` signature:**

```rust
pub fn skill_injection_middleware(
    config: &SkillsConfig,
) -> impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static
```

**Impact on FR-7 and the design:**
- FR-7 must specify the return type as `impl Fn(&Conversation) -> Option<Conversation> + ...`
  (not `-> Middleware`).
- Step 7 (wiring in main.rs) simply chains `.pre_turn(skill_injection_middleware(&config.skills))`
  onto the existing `shell_hook_bridge(...)` call.
- `Middleware::merge` is NOT needed and should NOT be added.
- FAIL-2 resolution stands: if `select_skills` returns empty, return `None` from the
  closure (leave `system_prompt` unchanged), with an explicit guard before calling
  `skills_to_system_prompt`.

All other requirements are unaffected.
