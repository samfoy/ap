# ap Development Backlog

This file drives the continuous development loop. The monitor agent reads this, picks the highest-priority incomplete item, writes a new PROMPT.md, and spawns a Ralph loop.

## Format
- `[ ]` = not started
- `[~]` = in progress (current Ralph loop)
- `[x]` = complete

---

## 🔴 Active

- [x] **FP Refactor** — Immutable Conversation, pure turn() pipeline, Rust middleware chain replacing shell hooks

---

## 🟠 Next Up (Priority Order)

1. [~] **Provider abstraction** — Clean Provider trait with easy swap. Add OpenAI-compatible provider (works with any OpenAI API endpoint — OpenRouter, LM Studio, Ollama). Config: `[provider] backend = "openai-compat" base_url = "..." api_key = "..."`. Streaming via SSE. Same tool call format as Bedrock adapter.

2. [ ] **Skill system** — ap discovers and loads "skills" from `~/.ap/skills/` and `./.ap/skills/`. A skill is a markdown file (`SKILL.md`) that gets injected into the system prompt when relevant. Skills can declare tools they need. Compatible with pi/claude AGENTS.md skill conventions. Discovery: semantic search over available skills to auto-inject relevant ones per turn.

3. [ ] **Tool discovery** — `ap` can discover available tools from a project's context (reads `AGENTS.md`, `tools.toml`, skill directories). Presents discovered tools to Claude alongside built-ins.

4. [ ] **Richer TUI** — Syntax highlighted code blocks in conversation pane. Tool call details expandable (press `e` on a tool result to expand). Token count + cost display in status bar. Scrollback history preserved across turns. Input: multi-line with `Ctrl+Enter` to submit, `Enter` for newline.

5. [ ] **Markdown + Mermaid rendering** — Render markdown in the conversation pane natively in the terminal:
    - Markdown: headings, bold/italic, inline code, fenced code blocks with syntax highlighting, bullet lists, numbered lists, blockquotes — rendered via `termimad` or `pulldown-cmark` + custom ratatui renderer
    - Mermaid diagrams: detect fenced ` ```mermaid ` blocks, render as ASCII art in-terminal using `mermaid-cli` (`mmdc`) if available, or fall back to raw source with a `[diagram]` label
    - Toggle: `m` key switches between rendered and raw markdown view
    - Code blocks: language-aware syntax highlighting via `syntect` crate

5. [ ] **Conversation context management** — Auto-summarize old messages when context window fills. `--context-limit` flag. Show context usage in TUI status bar.

6. [ ] **Image support** — Pass images to Claude via `@image.png` syntax in prompt (like pi). Base64 encode, attach as vision message.

7. [ ] **AGENTS.md support** — Load and inject agent context from both global and project level, same convention as pi/claude code:
    - **Global:** `~/.ap/AGENTS.md` — always injected, defines persona, coding style, preferences
    - **Project:** `./AGENTS.md` (cwd at startup) — injected after global, overrides/extends it
    - Both are injected into the system prompt at startup, global first then project
    - Hot reload: if `AGENTS.md` changes during a session, pick it up on next turn
    - Config: `[agents] global = "~/.ap/AGENTS.md"` (override path if needed), `project = true` (auto-discover from cwd, default on)
    - Skills referenced in AGENTS.md (`## Skills` section listing skill names) trigger skill loading from `~/.ap/skills/` or `./.ap/skills/`
    - Compatible with pi, claude code, and OpenClaw AGENTS.md conventions — same file works across all three

8. [ ] **Streaming improvements** — Show token-by-token streaming in TUI conversation pane (not batched). Interrupt streaming with `Ctrl+C` (cancel current turn, keep conversation).

9. [ ] **Background process management + tmux sub-agents** — Non-blocking process execution with TUI awareness:
    - **Background bash tool** — `bash` tool gains `background: true` param. Spawns process detached, returns a `job_id` immediately. Claude can continue the conversation while it runs.
    - **Jobs panel in TUI** — New right-side panel (or toggleable overlay, `j` key) showing running/completed background jobs: name, pid, status, runtime, last line of output
    - **Job alerts** — When a background job completes (or errors), a non-blocking notification appears in the TUI status bar. Claude is also notified via a synthetic tool result injected into the next turn: `{"job_id": "...", "exit_code": 0, "stdout_tail": "..."}`
    - **tmux sub-agents** — Built-in `tmux` awareness: `bash` tool can target a named tmux session/window (`tmux_target: "ap-worker"`) to run long commands visibly. `ap` knows how to create, attach, and read from tmux panes. Sub-agent pattern: spawn `ap --session worker -p "..."` in a tmux window, monitor its session file for completion.
    - **Job lifecycle:** `job list`, `job attach <id>` (open tmux pane), `job kill <id>`, `job logs <id>` — callable by Claude as tool calls or by user as `/job` commands in TUI input
    - Config: `[jobs] max_concurrent = 4, tmux_enabled = true, default_shell = "zsh"`

10. [ ] **Session management UX** — First-class session browsing and branching:
    - `ap sessions` — list recent sessions with id, date, turn count, and auto-generated title (from first message)
    - `ap --resume` — pick up the most recent session automatically
    - `ap --resume <id>` — resume a specific session by id or fuzzy name match
    - `ap --fork <id>` — branch from any point in a past session into a new one (copy messages up to that turn, new session id). Useful for trying a different approach without losing the original.
    - TUI: `s` key opens a session browser overlay — scrollable list, preview pane showing last few messages, Enter to resume, `f` to fork
    - Session titles: auto-generated from first user message (short LLM summary, cached in session file)
    - Sessions stored at `~/.ap/sessions/<id>.json` as before

11. [ ] **Semantic search over sessions + directories** — Built-in vector search, no external service required. Two search surfaces:
    - **Session memory**: index past `~/.ap/sessions/*.json` — search conversation history by meaning, auto-inject relevant past context into new sessions (`--recall` flag or always-on config)
    - **Directory search**: index configured paths (`[search] dirs = ["~/Documents", "./src"]`) for code and notes — expose as a built-in `search` tool Claude can call
    - Backend: local embeddings via `fastembed-rs` crate (all-MiniLM-L6-v2, runs on CPU, no API key). Index stored at `~/.ap/index/` as HNSW graph (using `instant-distance` or `usearch` crate)
    - Incremental indexing: watch for new sessions + file changes, reindex in background
    - Config: `[search] enabled = true, dirs = [], session_recall = true, recall_top_k = 3`
    - The `search` tool schema: `{ "query": string, "scope": "sessions" | "dirs" | "all", "top_k": number }`
    - Results injected as a system message block before the turn, labeled clearly so Claude knows the provenance

11. [ ] **LSP integration** — Connect to running language servers for code-aware context:
    - `lsp` built-in tool: `{ "op": "hover" | "definition" | "references" | "diagnostics" | "completion", "file": "...", "line": N, "col": N }`
    - ap spawns or connects to an existing LSP server (e.g. `rust-analyzer`, `pyright`, `typescript-language-server`) based on project language, detected from cwd
    - Results injected as tool output — Claude sees type info, diagnostics, go-to-def results as structured text
    - Diagnostics surface passively: on file write, ap runs `diagnostics` on the saved file and appends errors/warnings as a follow-up tool result
    - Config: `[lsp] enabled = true, servers = { rust = "rust-analyzer", python = "pyright", typescript = "typescript-language-server" }`
    - Uses `tower-lsp` client (Rust LSP client crate) or shells out to `lsp-cli` if available
    - TUI: diagnostics panel (toggleable, `d` key) shows current file errors inline — Built-in vector search, no external service required. Two search surfaces:
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

---

## 🏁 Milestone: Self-Hosting

> **ap builds ap.**
>
> When items 1 (provider abstraction), 7 (AGENTS.md), and 9 (background processes + tmux) are complete:
> - Switch `ap-monitor.py` from `pi --print` to `ap -p`
> - Switch Ralph loop backend from `pi` to `ap` (`ralph.yml`: `cli.backend: "ap"`)
> - `ap` is now the agent driving its own development loop
>
> Gate: `ap -p "read BACKLOG.md and summarize the next 3 items"` must work reliably end-to-end.