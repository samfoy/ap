# ap Development Backlog

This file drives the continuous development loop. The monitor agent reads this, picks the highest-priority incomplete item, writes a new PROMPT.md, and spawns a Ralph loop.

## Format
- `[ ]` = not started
- `[ ]` = in progress (current Ralph loop)
- `[x]` = complete

---

## 🔴 Active

- [x] **FP Refactor** — Immutable Conversation, pure turn() pipeline, Rust middleware chain replacing shell hooks

---

## 🟠 Next Up (Priority Order)

> **Bootstrap goal:** ap builds ap. Critical path: Provider abstraction → AGENTS.md → Self-hosting. Everything else is secondary until the loop flips.

0. [x] **Session persistence in --prompt mode** — `ap --prompt "..."` (non-interactive/headless) currently discards session history; it should save to `~/.ap/sessions/` just like interactive mode:
    - On startup in `--prompt` mode, create a named session (slug from prompt, e.g. `prompt-read-backlog-2026-03-22`) same as interactive
    - After turn completes, save session to `~/.ap/sessions/<name>.json`
    - `--session <name>` flag works in `--prompt` mode to explicitly name/resume a session
    - Stdout still receives the response (existing behavior preserved)
    - Test: `ap --prompt "hello"` creates a session file in `~/.ap/sessions/`

1. [x] **Provider abstraction** — Clean Provider trait with easy swap. Add OpenAI-compatible provider (works with any OpenAI API endpoint — OpenRouter, LM Studio, Ollama). Config: `[provider] backend = "openai-compat" base_url = "..." api_key = "..."`. Streaming via SSE. Same tool call format as Bedrock adapter.

2. [x] **AGENTS.md support** — Load and inject agent context from both global and project level, same convention as pi/claude code:
    - **Global:** `~/.ap/AGENTS.md` — always injected, defines persona, coding style, preferences
    - **Project:** `./AGENTS.md` (cwd at startup) — injected after global, overrides/extends it
    - Both are injected into the system prompt at startup, global first then project
    - Hot reload: if `AGENTS.md` changes during a session, pick it up on next turn
    - Config: `[agents] global = "~/.ap/AGENTS.md"` (override path if needed), `project = true` (auto-discover from cwd, default on)
    - Skills referenced in AGENTS.md trigger skill loading from `~/.ap/skills/` or `./.ap/skills/`
    - Compatible with pi, claude code, and OpenClaw AGENTS.md conventions

3. [x] **Self-hosting (ap builds ap)** — Switch the Ralph build loop from pi to ap itself:
    - Gate: `ap -p "read BACKLOG.md and summarize the next 3 items"` works reliably end-to-end
    - Update `ralph.yml` cli.backend from `pi` to `ap`
    - Update `ap-monitor.py` to use `ap --print` instead of `pi --print`
    - Requires: Provider abstraction + AGENTS.md + stable non-interactive mode
    - Milestone: ap is the agent driving its own development loop

4. [x] **Conversation context management** — Auto-summarize old messages when context window fills. `--context-limit` flag. Show context usage in TUI status bar.

5. [x] **Session management UX** — All sessions are named and persisted to disk from the first turn. No ephemeral/throwaway runs.
    - Every `ap` invocation creates a named session immediately — name auto-generated from first user message (short slug, e.g. `refactor-auth-module-2026-03-22`)
    - `--session <name>` to give an explicit name at startup
    - Sessions saved to `~/.ap/sessions/<name>.json` after every turn
    - `ap sessions` — list all sessions: name, date, turn count, last message snippet
    - `ap --resume` — resume most recent session
    - `ap --resume <name>` — resume by name or fuzzy match
    - `ap --fork <name>` — branch from a past session into a new named one
    - TUI: `s` key opens session browser overlay — scrollable list, preview pane, Enter to resume, `f` to fork
    - Remove the `--session` opt-in flag concept entirely — persistence is always on

6. [x] **Model switching** — Swap models mid-session without restarting. Config-driven + runtime toggle:
    - `/model <id>` command in TUI input switches active model immediately
    - `--model` CLI flag overrides config at startup
    - Model displayed in TUI status bar
    - Works across all providers (Bedrock, OpenAI-compat)
    - Recent models remembered in `~/.ap/models.json` for quick switching

7b. [ ] **TUI overhaul — simple Claude Code / pi style UI** — Current TUI is too busy and broken (Ctrl+Enter inserts newline instead of submitting). Replace with a clean, minimal chat UI:
    - **Input:** Single-line input bar at bottom (like pi/claude code). Enter submits. Shift+Enter or `\n` in input for newlines if needed. No multi-line editor widget.
    - **Chat area:** Scrollable message history above input. User messages right-aligned or prefixed `You:`, assistant left-aligned or prefixed `ap:`. Plain text rendering, no heavy layout.
    - **Status bar:** Single line at very top or bottom showing: model name, session name, token count. Nothing else.
    - **Remove:** Split panes, tool call panels, busy sidebars, any layout complexity that isn't the chat + status bar.
    - **Key bindings:** Enter = submit, Up/Down = scroll history, Ctrl+C = cancel/exit. That's it.
    - **Reference:** pi's TUI (single input line, clean scroll), Claude Code's terminal output style (assistant text streamed inline, no framed boxes).
    - Acceptance: `ap` launches, shows clean input prompt, user types, Enter submits, response streams in, feels like a normal terminal chat tool.

7. [x] **Kiro provider** — Add Kiro (AWS CodeWhisperer/Q) as a provider backend. Free access to 17 models including Claude Opus/Sonnet 4.6, DeepSeek 3.2, Kimi K2.5, Qwen3 Coder, GLM 4.7, and more. Auth via AWS Builder ID (SSO OIDC device code flow) or kiro-cli SQLite credential reuse.

    **API details** (from pi-provider-kiro reference impl at ~/Projects/pi-provider-kiro):
    - Endpoint: `https://q.us-east-1.amazonaws.com/generateAssistantResponse`
    - Auth: Bearer token (AWS SSO access token)
    - Request format: `{ conversationState: { currentMessage: { userInputMessage: { content, modelId, origin: "AI_EDITOR", images?, userInputMessageContext?: { toolResults?, tools? } } }, conversationId?, history? } }`
    - Response: SSE stream of `data: {...}` events — parse `generateAssistantResponseResponse` events for text/tool chunks
    - Tool calls: Kiro uses bracket format `[tool_name(param="val")]` in text stream — parse via bracket-tool-parser pattern
    - Thinking: wrapped in `<thinking>...</thinking>` tags in the text stream

    **Auth flow** (two methods):
    1. **kiro-cli reuse** — Read from `~/Library/Application Support/kiro-cli/data.sqlite3`, keys `kirocli:odic:token` (IDC) or `kirocli:social:token` (desktop). Parse JSON value for `access_token`, `refresh_token`, `expires_at`, `region`. Preferred if kiro-cli is installed.
    2. **Device code flow** — Register client at `https://oidc.us-east-1.amazonaws.com/client/register` → get device code at `/device_authorization` (startUrl: `https://view.awsapps.com/start`) → poll `/token` until user approves in browser. Scopes: `codewhisperer:completions`, `codewhisperer:analysis`, `codewhisperer:conversations`, `codewhisperer:transformations`, `codewhisperer:taskassist`.
    - Token refresh: POST to `https://oidc.us-east-1.amazonaws.com/token` with `grant_type=refresh_token`
    - Desktop token refresh: POST to `https://prod.{region}.auth.desktop.kiro.dev/refreshToken`

    **Model IDs** (use dot notation in API, config uses dashes): claude-opus-4.6, claude-opus-4.6-1m, claude-sonnet-4.6, claude-sonnet-4.6-1m, claude-opus-4.5, claude-sonnet-4.5, claude-sonnet-4.5-1m, claude-sonnet-4, claude-haiku-4.5, deepseek-3.2, kimi-k2.5, minimax-m2.1, glm-4.7, glm-4.7-flash, qwen3-coder-next, qwen3-coder-480b, agi-nova-beta-1m. All are zero-cost.

    **Rust implementation plan:**
    - `src/provider/kiro.rs` — `KiroProvider` implementing `Provider` trait
    - `src/auth/kiro.rs` — credential store, SQLite read (via `rusqlite`), device code flow, token refresh
    - `src/provider/kiro_transform.rs` — message/tool conversion to Kiro wire format, bracket tool call parser
    - Config: `[provider] backend = "kiro" model = "claude-sonnet-4.6"`
    - Credential persistence: `~/.ap/kiro-token.json` (mirrors kiro-cli DB on write-back)
    - `ap login kiro` — CLI subcommand to trigger device code flow and save token
    - SSE parsing: reuse existing streaming infrastructure, add Kiro-specific event parser

    **Reference:** ~/Projects/pi-provider-kiro/src/ — full TypeScript implementation to port from.

8. [x] **Code review + aggressive refactor pass** — Full codebase review and cleanup:
    - Audit all public APIs for consistency (naming, error types, return conventions)
    - Identify and eliminate any remaining mutable state outside the turn pipeline
    - Dead code removal, unused dependencies pruned from Cargo.toml
    - Benchmark turn() latency — identify any blocking calls that should be async
    - Review all error handling: replace any remaining panics with proper Results
    - Clippy pedantic pass: fix all warnings, document any intentional allows
    - Write architectural decision records (ADRs) for key design choices in docs/

9. [x] **Slack bot integration** — ap as a Slack bot, similar to pi-slack-bot:
    - Slash command or @mention triggers ap in any channel or DM
    - Streaming responses posted as editable Slack messages (updated chunk by chunk)
    - Tool calls shown as threaded replies (collapsible)
    - Session per Slack thread — conversation history maintained
    - Config: `[slack] bot_token = "..." app_token = "..." signing_secret = "..."`
    - Socket Mode for no-ingress-required deployment
    - Runs as a daemon: `ap slack-bot`

10. [x] **Background process management + tmux sub-agents** — Non-blocking process execution with TUI awareness:
    - **Background bash tool** — `bash` tool gains `background: true` param. Spawns process detached, returns a `job_id` immediately. Claude can continue the conversation while it runs.
    - **Jobs panel in TUI** — New right-side panel (or toggleable overlay, `j` key) showing running/completed background jobs: name, pid, status, runtime, last line of output
    - **Job alerts** — When a background job completes (or errors), a non-blocking notification appears in the TUI status bar. Claude is also notified via a synthetic tool result injected into the next turn: `{"job_id": "...", "exit_code": 0, "stdout_tail": "..."}`
    - **tmux sub-agents** — Built-in `tmux` awareness: `bash` tool can target a named tmux session/window (`tmux_target: "ap-worker"`) to run long commands visibly. `ap` knows how to create, attach, and read from tmux panes. Sub-agent pattern: spawn `ap --session worker -p "..."` in a tmux window, monitor its session file for completion.
    - **Job lifecycle:** `job list`, `job attach <id>` (open tmux pane), `job kill <id>`, `job logs <id>` — callable by Claude as tool calls or by user as `/job` commands in TUI input
    - Config: `[jobs] max_concurrent = 4, tmux_enabled = true, default_shell = "zsh"`

11. [x] **Streaming improvements** — Show token-by-token streaming in TUI conversation pane (not batched). Interrupt streaming with `Ctrl+C` (cancel current turn, keep conversation).

12. [x] **Semantic search over sessions + directories** — Built-in vector search, no external service required. Two search surfaces:
    - **Session memory**: index past `~/.ap/sessions/*.json` — search conversation history by meaning, auto-inject relevant past context into new sessions (`--recall` flag or always-on config)
    - **Directory search**: index configured paths (`[search] dirs = ["~/Documents", "./src"]`) for code and notes — expose as a built-in `search` tool Claude can call
    - Backend: local embeddings via `fastembed-rs` crate (all-MiniLM-L6-v2, runs on CPU, no API key). Index stored at `~/.ap/index/` as HNSW graph (using `instant-distance` or `usearch` crate)
    - Incremental indexing: watch for new sessions + file changes, reindex in background
    - Config: `[search] enabled = true, dirs = [], session_recall = true, recall_top_k = 3`
    - The `search` tool schema: `{ "query": string, "scope": "sessions" | "dirs" | "all", "top_k": number }`
    - Results injected as a system message block before the turn, labeled clearly so Claude knows the provenance

13. [x] **LSP integration** — Connect to running language servers for code-aware context:
    - `lsp` built-in tool: `{ "op": "hover" | "definition" | "references" | "diagnostics" | "completion", "file": "...", "line": N, "col": N }`
    - ap spawns or connects to an existing LSP server based on project language, detected from cwd
    - Results injected as tool output
    - Diagnostics surface passively: on file write, ap runs `diagnostics` on the saved file and appends errors/warnings as a follow-up tool result
    - Config: `[lsp] enabled = true, servers = { rust = "rust-analyzer", python = "pyright", typescript = "typescript-language-server" }`
    - TUI: diagnostics panel (toggleable, `d` key) shows current file errors inline

14. [x] **Skill system** — ap discovers and loads "skills" from `~/.ap/skills/` and `./.ap/skills/`. Already merged.

15. [x] **Tool discovery** — Already merged.

16. [x] **Richer TUI** — Already merged.

17. [x] **Markdown + Mermaid rendering** — Render markdown in the conversation pane natively in the terminal.

18. [x] **Image support** — Pass images to Claude via `@image.png` syntax in prompt (like pi). Base64 encode, attach as vision message.


19. [x] **Robust file editing** — Reliable, no-friction file edits by default:
    - **No approval prompts by default** — edits apply immediately, no confirmation dialogs. Trust the user. `--safe` flag available for those who want confirmation.
    - **Dry-run / diff preview** — `--dry-run` flag shows unified diff without writing. Claude can also call `preview_edit` tool to show before applying.
    - **Atomic multi-file apply** — all edits in a turn applied as a batch; if any write fails, roll back the whole set
    - **Undo** — `/undo` command reverts last batch of edits (saves pre-edit snapshot in `~/.ap/undo/`)
    - **Large file safety** — for files >1000 lines, use line-range context in the edit tool rather than full-file replacement; validate line numbers against actual file before applying
    - **Edit tool schema:** `{ "file": string, "old_str": string, "new_str": string }` (exact match replace, not line numbers) — same convention as Claude Code / pi
    - **Write tool:** `{ "file": string, "content": string }` — full file overwrite, always works
    - Config: `[editing] require_approval = false` (default)

20. [x] **Amazon toolchain integration** — First-class support for Ada and AWS credentials/SDK:
    - **Ada credential management** — detect and use Ada-vended credentials automatically. Run `ada credentials update --provider isengard --account <id> --role <role>` when creds are expired. ap knows to re-credential and retry if AWS calls return 401/403.
    - **AWS CLI/SDK awareness** — `aws` tool built-in: Claude can call AWS CLI commands directly as a tool call. Results returned as structured JSON where possible.
    - **Profile detection** — auto-detect active AWS profile from environment (`AWS_PROFILE`, `~/.aws/config`), surface in TUI status bar
    - **Brazil awareness** — recognize Brazil package structure, understand `brazil-build`, `brazil ws`, `brazil-recursive-cmd` patterns
    - **Common patterns:** `ap "what's failing in my last brazil build"` auto-finds the build log; `ap "get the CloudWatch logs for service X"` uses current AWS profile
    - Config: `[aws] ada_enabled = true, default_profile = "auto"`

21. [ ] **Pi/Agent Skills compatibility** — Full support for the [Agent Skills standard](https://agentskills.io/specification) and pi skill conventions:

    **Discovery (expand current flat `.md` loader):**
    - Support subdirectory skills: scan for `SKILL.md` recursively under each skills dir (e.g. `~/.ap/skills/my-skill/SKILL.md`)
    - Flat `.md` files remain supported (backwards compat)
    - Skill locations: `~/.ap/skills/` (global), `.ap/skills/` (project, cwd + ancestors up to git root)
    - `--skill <path>` CLI flag (repeatable) to load a specific skill file or directory
    - `--no-skills` flag to disable auto-discovery (explicit `--skill` still loads)

    **Frontmatter (upgrade current parser):**
    - Parse `name` field — use as skill identifier (currently uses filename stem)
    - Parse `description` field — use for TF-IDF matching (currently uses full body)
    - Validate: name must be lowercase letters/numbers/hyphens, max 64 chars, match parent dir
    - Validate: description required (skip skill if missing), max 1024 chars
    - Warn on violations but still load (lenient per spec)
    - Existing `tools:` frontmatter key retained

    **Associated scripts + relative paths:**
    - Resolve relative paths (scripts/, references/, assets/) against the skill directory
    - Agent can read/exec `./scripts/foo.sh` and ap resolves correctly against skill dir

    **Skill commands:**
    - `/skill:<name>` command in TUI — force-loads named skill, appends args as `User: <args>`
    - Config: `skills.enable_commands = true` (default true)

    **Injection change:**
    - Inject only `name` + `description` in system prompt (progressive disclosure)
    - Full SKILL.md body loaded on demand via `read` tool when agent uses it
    - Replace current full-body injection

    **Reference:** pi skill spec at `~/Projects/pi-knowledge-search/node_modules/@mariozechner/pi-coding-agent/docs/skills.md`

22. [x] **Project-level config** — Per-project `ap.toml` that overlays global config:
    - Locations: `~/.ap/config.toml` (global) and `.ap/config.toml` (project, cwd + ancestors up to git root)
    - Project config discovered automatically at startup; applied after global
    - Deep merge for nested tables (e.g. `[context]`, `[skills]`), scalar values override
    - Allows per-project model, provider, context limit, skills paths, tool settings
    - CLI flags still take highest priority (global → project → CLI)
    - `--no-project-config` flag to skip project discovery
    - Reference: pi's global `~/.pi/agent/settings.json` + project `.pi/settings.json` pattern

23. [ ] **Prompt templates** — Expandable markdown snippets invoked as `/name` commands:
    - Locations: `~/.ap/prompts/*.md` (global), `.ap/prompts/*.md` (project)
    - Filename without `.md` becomes the command name (`review.md` → `/review`)
    - Frontmatter: optional `description` field; falls back to first non-empty line
    - Template body expands into the input editor when invoked
    - Argument support: `$1`, `$2`, `$@` positional args (e.g. `/review src/main.rs`)
    - TUI: typing `/` shows autocomplete dropdown of available templates with descriptions
    - `--prompt-template <path>` CLI flag (repeatable) for explicit loading
    - `--no-prompt-templates` to disable discovery
    - Reference: pi prompt templates at `~/Projects/pi-knowledge-search/node_modules/@mariozechner/pi-coding-agent/docs/prompt-templates.md`

24. [ ] **Retry with exponential backoff** — Automatic retry on transient provider errors:
    - Retry on: HTTP 429 (rate limit), 5xx server errors, network timeouts, SSE stream drops
    - Exponential backoff: base 2s, doubling each attempt (2s → 4s → 8s → ...)
    - Configurable: `[retry] enabled = true`, `max_retries = 3`, `base_delay_ms = 2000`, `max_delay_ms = 60000`
    - Respect `Retry-After` header when present
    - If provider requests delay > `max_delay_ms` (e.g. quota reset in 5h), fail immediately with clear error
    - Show retry status in TUI status bar: `retrying (2/3)...`
    - Non-retryable errors (4xx auth, invalid request) fail immediately — no retry

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