---
status: completed
created: 2026-03-22
started: 2026-03-22
completed: 2026-03-22
---
# Task: Implement select_skills TF-IDF and skills_to_system_prompt

## Description
Extend `src/skills/mod.rs` with `select_skills()` (TF-IDF relevance scoring) and `skills_to_system_prompt()` (formatter). `select_skills` returns the top-N skills by TF-IDF score, excluding any with score 0. `skills_to_system_prompt` formats them into a Markdown block suitable for injection as a system prompt.

## Background
TF-IDF is computed fully in pure Rust — no ML crates. The query is the concatenated text of all conversation messages. Each skill's body is one document in the corpus. Skills that don't share any tokens with the query score 0 and are excluded.

## Reference Documentation
**Required:**
- Design: .agents/scratchpad/implementation/skill-system/design.md (Sections 2 FR-5/FR-6, Section 4.4/4.5, Appendix C.2)

**Additional References:**
- .agents/scratchpad/implementation/skill-system/context.md (Message type shape)
- .agents/scratchpad/implementation/skill-system/plan.md (Step 4)

**Note:** You MUST read the design document before beginning implementation.

## Technical Requirements
1. `pub fn select_skills<'a>(skills: &'a [Skill], messages: &[Message], max_n: usize) -> Vec<&'a Skill>`
   - Tokenize: lowercase, split on non-alphanumeric characters, filter empty tokens
   - For each skill, compute TF-IDF score against query tokens
   - TF = (term count in skill body) / (total tokens in skill body)
   - IDF = ln(N / df + 1) where N = total skills, df = number of skills containing the term
   - Score per skill = sum of TF * IDF for each query token present in skill
   - Exclude skills with score == 0.0 (f64 equality is safe here — a score of 0 means no shared tokens)
   - Return top `max_n` skills sorted by descending score
2. `pub fn skills_to_system_prompt(skills: &[&Skill]) -> String`
   - Format: `"## Skills\n\n### {name}\n{body}\n"` for each skill, joined
   - Callers must not pass empty slice (the empty-guard is in `skill_injection_middleware`)
3. Empty messages → return empty vec (no scoring attempted)
4. No new crates

## Dependencies
- Task 03 (Step 3): `Skill` struct and `Message` import must exist

## Implementation Approach
1. **RED**: Write 4 failing tests:
   - `select_skills_returns_top_n`: 3 skills, query matches 2, `max_n=1` → highest scorer only
   - `select_skills_excludes_zero_score`: 2 skills, query matches only 1 → 1 skill returned
   - `select_skills_empty_messages`: empty messages → empty vec
   - `skills_to_system_prompt_format`: `[Skill { name:"foo", body:"bar", ..}]` → `"## Skills\n\n### foo\nbar\n"`
2. **GREEN**: Implement both functions
3. **REFACTOR**: Extract `tokenize(text: &str) -> Vec<String>` helper; ensure no allocations in hot paths beyond what's needed

## Acceptance Criteria

1. **Top-N returned correctly**
   - Given 3 skills where skills A and B share tokens with the query but C does not, and `max_n=1`
   - When `select_skills` is called
   - Then returns only the highest-scoring skill

2. **Zero-score skills excluded**
   - Given 2 skills where only one shares tokens with the query
   - When `select_skills` is called with `max_n=5`
   - Then returns exactly 1 skill

3. **Empty messages returns empty**
   - Given an empty `messages` slice
   - When `select_skills` is called
   - Then returns an empty `Vec`

4. **Formatter produces correct output**
   - Given `[Skill { name: "foo", body: "bar\n", tools: [] }]`
   - When `skills_to_system_prompt` is called
   - Then output equals `"## Skills\n\n### foo\nbar\n"`

5. **Unit Tests Pass**
   - Given the implementation is complete
   - When running `cargo test -- select_skills skills_to_system_prompt`
   - Then all 4 tests pass

## Metadata
- **Complexity**: Medium
- **Labels**: skills, tfidf, scoring, formatter
- **Required Skills**: Rust, algorithms, string formatting
