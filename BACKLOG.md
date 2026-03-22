# ap Development Backlog

This file drives the continuous development loop. The monitor agent reads this, picks the highest-priority incomplete item, writes a new PROMPT.md, and spawns a Ralph loop.

## Format
- `[ ]` = not started
- `[~]` = in progress (current Ralph loop)
- `[x]` = complete

---

## 🔴 Active

- [~] **FP Refactor** — Immutable Conversation, pure turn() pipeline, Rust middleware chain replacing shell hooks

---

## 🟠 Next Up (Priority Order)

1. [ ] **Provider abstraction** — Clean Provider trait with easy swap. Add OpenAI-compatible provider (works with any OpenAI API endpoint — OpenRouter, LM Studio, Ollama). Config: `[provider] backend = "openai-compat" base_url = "..." api_key = "..."`. Streaming via SSE. Same tool call format as Bedrock adapter.

2. [ ] **Skill system** — ap discovers and loads "skills" from `~/.ap/skills/` and `./.ap/skills/`. A skill is a markdown file (`SKILL.md`) that gets injected into the system prompt when relevant. Skills can declare tools they need. Compatible with pi/claude AGENTS.md skill conventions. Discovery: semantic search over available skills to auto-inject relevant ones per turn.

3. [ ] **Tool discovery** — `ap` can discover available tools from a project's context (reads `AGENTS.md`, `tools.toml`, skill directories). Presents discovered tools to Claude alongside built-ins.

4. [ ] **Richer TUI** — Syntax highlighted code blocks in conversation pane. Tool call details expandable (press `e` on a tool result to expand). Token count + cost display in status bar. Scrollback history preserved across turns. Input: multi-line with `Ctrl+Enter` to submit, `Enter` for newline.

5. [ ] **Conversation context management** — Auto-summarize old messages when context window fills. `--context-limit` flag. Show context usage in TUI status bar.

6. [ ] **Image support** — Pass images to Claude via `@image.png` syntax in prompt (like pi). Base64 encode, attach as vision message.

7. [ ] **Project awareness** — On startup, read `AGENTS.md` / `CLAUDE.md` / `.cursorrules` from cwd and inject as system context. Auto-detect language/framework.

8. [ ] **Streaming improvements** — Show token-by-token streaming in TUI conversation pane (not batched). Interrupt streaming with `Ctrl+C` (cancel current turn, keep conversation).

9. [ ] **Session management UX** — `ap sessions` command lists recent sessions with summaries. `ap --resume` picks up the most recent session automatically. Session names: auto-generated from first message.

10. [ ] **Semantic search over sessions + directories** — Built-in vector search, no external service required. Two search surfaces:
    - **Session memory**: index past `~/.ap/sessions/*.json` — search conversation history by meaning, auto-inject relevant past context into new sessions (`--recall` flag or always-on config)
    - **Directory search**: index configured paths (`[search] dirs = ["~/Documents", "./src"]`) for code and notes — expose as a built-in `search` tool Claude can call
    - Backend: local embeddings via `fastembed-rs` crate (all-MiniLM-L6-v2, runs on CPU, no API key). Index stored at `~/.ap/index/` as HNSW graph (using `instant-distance` or `usearch` crate)
    - Incremental indexing: watch for new sessions + file changes, reindex in background
    - Config: `[search] enabled = true, dirs = [], session_recall = true, recall_top_k = 3`
    - The `search` tool schema: `{ "query": string, "scope": "sessions" | "dirs" | "all", "top_k": number }`
    - Results injected as a system message block before the turn, labeled clearly so Claude knows the provenance

---

## ✅ Complete

- [x] Initial v1 build (scaffold, config, tools, provider, hooks, TUI, non-interactive, README)
- [x] Extensions system removed (agent-era philosophy: fork the code)
