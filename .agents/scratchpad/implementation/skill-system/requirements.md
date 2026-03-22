# Requirements: Skill System for `ap`

## Vision

Offline, pure-Rust skill injection using TF-IDF relevance scoring.
No embedding APIs, no ML dependencies. Skills are markdown files.

## Functional Requirements

### FR-1: `Conversation.system_prompt` — transient field

- `Conversation` gains `system_prompt: Option<String>`
- Annotated `#[serde(skip)]` — **not persisted** to session files
- Each session starts with `None`; middleware sets it on every turn
- Builder: `Conversation::with_system_prompt(prompt: impl Into<String>) -> Self`
- **Rationale**: Skills are derived/computed content. Persisting a scored
  snapshot would lock in stale TF-IDF results from a prior session's messages.

### FR-2: Provider system prompt threading

- `Provider::stream_completion` gains `system_prompt: Option<&str>` parameter
- `BedrockProvider::build_request_body` accepts `system_prompt: Option<&str>`;
  injects `"system": text` into the JSON body when `Some`
- `turn()` passes `conv.system_prompt.as_deref()` to the provider

### FR-3: Skill files

- Skills live in `~/.ap/skills/` (global) and `./.ap/skills/` (project)
- Each skill is a `.md` file; filename (sans extension) is the skill name
- Optional YAML-lite frontmatter block at top of file:
  ```
  ---
  tools: [bash, read]
  ---
  ```
  Only `tools:` key is parsed; content below frontmatter is the skill body
- Project skills override global skills with the same name (later wins)

### FR-4: `Skill` and `SkillLoader` types (`src/skills/mod.rs`)

```rust
pub struct Skill {
    pub name: String,
    pub body: String,
    pub tools: Vec<String>,
}

pub struct SkillLoader {
    dirs: Vec<PathBuf>,
}

impl SkillLoader {
    pub fn new(dirs: Vec<PathBuf>) -> Self;
    pub fn load(&self) -> Vec<Skill>;
}
```

`load()` merges directories with later-wins semantics by skill name.

### FR-5: TF-IDF skill selection

```rust
pub fn select_skills<'a>(
    skills: &'a [Skill],
    messages: &[Message],
    max_n: usize,
) -> Vec<&'a Skill>
```

- Corpus: one document per skill (body text)
- Query: concatenated text of all conversation messages
- Returns up to `max_n` skills by descending TF-IDF score
- Skills with score 0 are excluded
- Tokenization: lowercase, split on non-alphanumeric characters

### FR-6: System prompt formatter

```rust
pub fn skills_to_system_prompt(skills: &[&Skill]) -> String
```

- Produces a formatted block suitable for injection as the system prompt
- Example format:
  ```
  ## Skills

  ### skill-name
  <body>
  ```

### FR-7: `skill_injection_middleware()` — pre_turn closure

```rust
pub fn skill_injection_middleware(
    loader: SkillLoader,
    config: SkillsConfig,
) -> impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static
```

- Returns a **closure** (not a `Middleware` struct) compatible with `Middleware::pre_turn()`
- The closure:
  1. Calls `loader.load()` to get skills
  2. Calls `select_skills()` with conversation messages and `config.max_injected`
  3. **If `select_skills` returns empty** — returns `None` (leave conversation unchanged; do NOT call `skills_to_system_prompt` with an empty slice)
  4. Calls `skills_to_system_prompt()` on the non-empty result
  5. **Sets** `conv.system_prompt = Some(block)` (never appends; field is always `None` on entry due to `#[serde(skip)]`)
  6. Returns `Some(mutated_conversation)`
- **Rationale**: the `Middleware::pre_turn()` builder already accepts this closure type;
  returning a full `Middleware` struct would add unnecessary API surface and invite a
  `merge`-footgun. A closure passed to a named slot is unambiguous.

### FR-8: `SkillsConfig` in `AppConfig`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    pub enabled: bool,                     // default: true
    pub max_injected: usize,               // default: 5
    pub global_dir: Option<PathBuf>,       // default: ~/.ap/skills/
    pub project_dir: Option<PathBuf>,      // default: ./.ap/skills/
}
```

- `AppConfig` gains `pub skills: SkillsConfig`

### FR-9: Wiring in `main.rs`

- Both `run_headless` and `run_tui` build a `SkillLoader` from config dirs
- Chain the skill closure via the existing builder:
  ```rust
  let middleware = shell_hook_bridge(&config.hooks)
      .pre_turn(skill_injection_middleware(loader, config.skills.clone()));
  ```
- If `skills.enabled == false`, the `.pre_turn(...)` call is skipped
- **No `Middleware::merge` method is needed or added**

## Non-Functional Requirements

- **No new external crates** beyond what already exists (pure Rust, no ML crates)
- Each step must compile and pass `cargo test` independently
- Clippy clean (`cargo clippy --all-targets -- -D warnings`)
- Integration test: load skills from tempdir, run a turn, assert `system_prompt` contains expected skill

## Out of Scope

- Semantic/embedding-based scoring (explicitly deferred)
- Hot reload of skill files mid-session
- AGENTS.md `## Skills` section auto-loading (separate feature)
- Streaming/incremental skill injection

## Resolved Decisions

| Decision | Chosen | Rationale |
|----------|--------|-----------|
| `system_prompt` persistence | `#[serde(skip)]` transient | Skills are derived from runtime context; persisting stale scores causes accumulation bugs across sessions |
| Middleware behaviour | Set (replace) | Field always `None` on load; no append logic needed |
| `skill_injection_middleware` return type | `impl Fn(...)` closure | `Middleware::pre_turn()` already accepts this; returning a full `Middleware` struct adds surface area and a potential `merge`-footgun |
| Empty `select_skills` result | Return `None` (no injection) | Injecting an empty `"## Skills\n\n"` header is useless noise; guard before calling `skills_to_system_prompt` |
