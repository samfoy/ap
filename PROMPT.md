# PROMPT.md — ap Full Backlog Implementation

You are implementing features for `ap`, a Rust/ratatui AI coding agent located at `~/Projects/ap/ap/`.

## Project conventions
- Functional-first Rust: pure functions, immutable data, iterator chains
- Core types: `Conversation` (immutable), `turn() -> Result<(Conversation, Vec<TurnEvent>)>`, `Middleware` chain
- Every step must leave `cargo build` + `cargo test` + `cargo clippy -- -D warnings` passing before proceeding
- Commit after each item: `git add -A && git commit -m "feat(<slug>): implement <title>"`
- Work in `~/Projects/ap/ap/` (the Rust crate)

Read `BACKLOG.md` at `~/Projects/ap/BACKLOG.md` for full specs on each item.

## Items to implement in order

Work through each item completely before moving to the next. After completing each item, update `BACKLOG.md` to mark it `[x]` and commit.

---

### Item 1 — OpenAI-compatible provider

Add `src/provider/openai.rs` implementing the `Provider` trait using OpenAI-compatible HTTP/SSE.

Config:
```toml
[provider]
backend = "openai-compat"
base_url = "https://api.openai.com/v1"
api_key = "sk-..."
model = "gpt-4o"
```

- POST to `{base_url}/chat/completions` with `stream: true`
- Parse SSE `data:` lines → `StreamEvent`
- Tool calls via OpenAI function calling format
- Wire into `AppConfig` backend dispatch in `main.rs`
- Test: mock server returning SSE chunks

---

### Item 6 — Model switching

Add `/model <id>` command in TUI input to swap the active model mid-session.

- Parse `/model <id>` in TUI input handler (`src/tui/mod.rs`)
- Update `Conversation.config.provider.model` (or add a runtime override field)
- Display active model in status bar (already has ctx display — add `│ model: <id>`)
- `--model <id>` CLI flag overrides config at startup
- Save last 5 used models to `~/.ap/models.json` for quick recall
- Test: `/model` command updates model field in conversation

---

### Item 11 — Token-by-token streaming

Show each text token as it arrives in the TUI conversation pane.

- Currently batching: `TextDelta` events are accumulated then rendered
- Change: each `TurnEvent::TextDelta` sent over the channel triggers an immediate re-render
- Add partial `AssistantStreaming` chat entry type that gets replaced by `AssistantDone` on completion
- `Ctrl+C` during streaming cancels the in-flight request (abort the tokio task, keep conversation)
- Test: streaming entry appears and updates incrementally

---

### Item 19 — Robust file editing

Make file edits reliable with no approval prompts.

- Current `edit.rs` tool: make it the default (no confirmation dialogs)
- Add `--safe` flag to `AppConfig` / CLI that re-enables confirmation
- Dry-run: `preview_edit` tool shows unified diff without writing
- Atomic multi-file apply: collect all edits in a turn, apply as batch; rollback on any failure
- Undo: `/undo` TUI command reverts last batch (save pre-edit snapshots to `~/.ap/undo/`)
- Large file safety: for files >1000 lines use line-range context, validate line numbers before applying
- Test: edit tool applies without confirmation; `/undo` reverts

---

### Item 17 — Markdown rendering

Render markdown in the conversation pane natively.

- Use `pulldown-cmark` (add to Cargo.toml) to parse markdown
- Convert to ratatui `Line`/`Span` with Rose Pine theme colors:
  - Headings → `theme.md_heading` + bold
  - Code spans → `theme.code_fg` 
  - Code blocks → already handled in ui.rs, refine
  - Bold/italic → `Modifier::BOLD` / `Modifier::ITALIC`
  - Lists → `theme.md_heading` bullet + text
  - Links → `theme.accent` for link text
- Mermaid blocks: render as styled code block with `[mermaid]` label (no graphical render needed)
- Test: markdown string converts to correct styled lines

---

### Item 18 — Image support

Allow `@path/to/image.png` syntax in prompts to attach images.

- Parse `@<path>` tokens in user input before sending to provider
- Base64-encode the image file
- Add `MessageContent::Image { media_type: String, data: String }` variant
- Wire into Bedrock provider (already supports vision via `image` content blocks)
- Wire into OpenAI provider (base64 `image_url` content)
- TUI: show `[image: filename.png]` placeholder in conversation pane for attached images
- Test: `@image.png` in prompt adds image content to message

---

### Item 23 — Prompt templates

`/name` in TUI expands stored markdown snippets.

- Locations: `~/.ap/prompts/*.md` (global), `.ap/prompts/*.md` (project, cwd)
- Frontmatter: `description` field; falls back to first non-empty line
- Load templates at startup, expose as autocomplete in TUI input
- Typing `/` shows dropdown of available templates with descriptions
- Arguments: `$1`, `$2`, `$@` in template body get replaced with args typed after `/name arg1 arg2`
- `--no-prompt-templates` flag to disable
- Test: template expands with argument substitution

---

### Item 24 — Retry with exponential backoff

Automatic retry on transient provider errors.

- Retry on: HTTP 429, 5xx, network timeout, SSE stream drop
- Base delay 2s, doubles each attempt: 2s → 4s → 8s
- Config: `[retry] enabled = true`, `max_retries = 3`, `base_delay_ms = 2000`, `max_delay_ms = 60000`
- Respect `Retry-After` header
- If requested delay > `max_delay_ms`, fail immediately with clear error
- TUI status bar shows `retrying (2/3)...` during retry
- Non-retryable (401, 400): fail immediately
- Add `TurnEvent::Retrying { attempt: u32, max: u32, delay_ms: u64 }`
- Test: mock provider returning 429 triggers retry with backoff

---

### Item 21 — Pi/Agent Skills compatibility

Upgrade skill system to full Agent Skills spec.

- Subdirectory skills: scan for `SKILL.md` recursively under each skills dir
  (e.g. `~/.ap/skills/my-skill/SKILL.md`)
- Parse `name` and `description` frontmatter fields (currently only uses `tools:`)
- Validate name: lowercase letters/numbers/hyphens, max 64 chars, match parent dir — warn but load
- Progressive disclosure: inject only `name + description` in system prompt; full body loaded on-demand via `read` tool
- Relative paths in skill content resolved against skill directory
- `/skill:<name>` TUI command force-loads the named skill
- Config: `skills.enable_commands = true` (default)
- `--skill <path>` CLI flag (repeatable), `--no-skills` to disable
- Test: subdirectory SKILL.md discovered; description-only injection; `/skill:name` loads full body

---

### Item 10 — Background process management

Non-blocking bash tool with job tracking.

- `bash` tool gains optional `"background": true` param — spawns detached, returns `{"job_id": "..."}` immediately
- `job_list`, `job_logs`, `job_kill`, `job_attach` tools for managing background jobs
- Jobs stored in `~/.ap/jobs/` (pid file + log file per job)
- `TurnEvent::JobCompleted { job_id, exit_code, stdout_tail }` injected on next turn when job finishes
- TUI: `j` key toggles jobs panel showing running/completed jobs with status + last output line
- Test: background bash job runs and completion event fires

---

### Item 7 — Kiro provider

Add AWS Kiro (CodeWhisperer/Q) as a zero-cost provider backend.

Reference implementation: `~/Projects/pi-provider-kiro/src/`

- `src/provider/kiro.rs` implementing `Provider` trait
- `src/auth/kiro.rs`: credential store, SQLite read from kiro-cli DB, device code flow, token refresh
- API endpoint: `https://q.us-east-1.amazonaws.com/generateAssistantResponse`
- Auth: Bearer token (AWS SSO access token)
- SSE parsing: `generateAssistantResponseResponse` events
- Tool calls: bracket format `[tool_name(param="val")]` — parse via regex
- Config: `[provider] backend = "kiro" model = "claude-sonnet-4.6"`
- `ap login kiro` subcommand for device code flow
- Credential persistence: `~/.ap/kiro-token.json`
- Test: mock Kiro SSE response parses correctly

---

### Item 12 — Semantic search

Built-in vector search over sessions and directories.

- Add `fastembed` crate (local embeddings, all-MiniLM-L6-v2, CPU-only)
- Index: HNSW graph stored at `~/.ap/index/` using `usearch` or `instant-distance` crate
- Session recall: index `~/.ap/sessions/*.json` — search by meaning, inject top-k into new sessions
- Directory search: index configured paths (`[search] dirs = [...]`)
- Built-in `search` tool: `{ "query": string, "scope": "sessions"|"dirs"|"all", "top_k": number }`
- Incremental indexing: on startup, index any new/changed files
- Config: `[search] enabled = true`, `dirs = []`, `session_recall = true`, `recall_top_k = 3`
- Test: documents indexed and retrieved by semantic query

---

### Item 13 — LSP integration

Connect to language servers for code-aware context.

- `lsp` built-in tool: `{ "op": "hover"|"definition"|"references"|"diagnostics"|"completion", "file": "...", "line": N, "col": N }`
- Spawn or connect to LSP server based on detected project language (from cwd file extensions)
- Config: `[lsp] enabled = true`, `servers = { rust = "rust-analyzer", python = "pyright" }`
- Diagnostics panel in TUI: `d` key toggles, shows errors/warnings for current project
- On file write: run diagnostics, inject results as follow-up tool result
- Test: LSP hover returns type info for a known file

---

### Item 9 — Slack bot

ap as a Slack bot.

- `ap slack-bot` daemon subcommand
- Socket Mode (no public ingress needed)
- @mention or slash command triggers ap with the message content
- Streaming responses: update Slack message chunk by chunk
- Session per Slack thread — conversation history maintained in `~/.ap/sessions/`
- Config: `[slack] bot_token = "..."`, `app_token = "..."`, `signing_secret = "..."`
- Test: mock Slack event triggers ap response

---

### Item 20 — Amazon toolchain

Ada credentials + AWS CLI + Brazil awareness.

- Detect expired AWS credentials (401/403 on AWS calls) and run `ada credentials update --provider isengard --account <id> --role <role>` automatically
- `aws` built-in tool: Claude calls AWS CLI commands, results returned as structured JSON
- Auto-detect active AWS profile from `AWS_PROFILE` env / `~/.aws/config`; show in TUI status bar
- Brazil awareness: recognize `brazil-build`, `brazil ws`, `brazil-recursive-cmd` in bash outputs; surface errors clearly
- Config: `[aws] ada_enabled = true`, `default_profile = "auto"`
- Test: expired credential error triggers ada re-credential

---

## Completion

After ALL items above are implemented, verified with `cargo build --release`, `cargo test`, and `cargo clippy -- -D warnings`:

1. Update `~/Projects/ap/BACKLOG.md` — mark all implemented items `[x]`
2. `git add -A && git commit -m "chore: mark all items complete"`
3. `git push origin main`
4. Output: `LOOP_COMPLETE`
