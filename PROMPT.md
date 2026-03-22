The `PROMPT.md` is written to `/Users/sam.painter/Projects/ap/PROMPT.md`. Here's a summary of what it covers:

**Vision**: Offline, pure-Rust skill injection using TF-IDF relevance scoring — no embedding APIs, no ML deps.

**8 ordered implementation steps**, each independently compilable:
1. Extend `Conversation` with `system_prompt: Option<String>` + builder
2. Thread `system_prompt` through the `Provider` trait and `BedrockProvider` → Bedrock `"system"` field
3. `src/skills/mod.rs` — `Skill`, `SkillLoader` with frontmatter parsing (YAML-lite for `tools:`)
4. `select_skills()` TF-IDF scoring + `skills_to_system_prompt()` formatter
5. `skill_injection_middleware()` — the `pre_turn` closure that scores and injects
6. `SkillsConfig` in `AppConfig` (max_injected, dir overrides, overlay logic)
7. Wire into both `run_headless` and `run_tui` in `main.rs`
8. Integration test + clippy/doc polish

**Key design decisions reflected from the codebase**:
- Skills inject via the existing `Middleware::pre_turn` chain — no new extension points needed
- `system_prompt` lives on `Conversation` so it flows through the pure `turn()` function transparently
- Later directories (project) override earlier ones (global) by skill name — consistent with the existing config layering model
- The `Provider` trait change is the most invasive step, placed early so downstream steps don't accumulate breakage