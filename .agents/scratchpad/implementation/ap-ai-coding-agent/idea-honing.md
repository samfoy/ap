# Idea Honing — ap AI Coding Agent

## Session: 2026-03-22

### Context
The objective spec is detailed and comprehensive. Key areas that need clarification before design:
- Hooks cancellation protocol (how does pre_tool_call signal "cancel"?)
- Streaming behavior in non-interactive (`-p`) mode
- Concurrent/parallel tool call support
- Correct Bedrock model ID format

### Questions & Answers

---

**Q1 (2026-03-22):** When a `pre_tool_call` hook exits with a non-zero exit code, does that cancel the tool call? And if so, should `ap` report back to Claude that the tool was cancelled (e.g., as a tool_result with an error message), or silently skip it? What should the hook be able to inject into the cancellation reason?

**A1 (2026-03-22):** Yes — a non-zero exit from a `pre_tool_call` hook cancels the tool call. The protocol is:

1. **Cancellation signal:** Exit code != 0 means "cancel this tool call." Exit code 0 means "proceed."

2. **What Claude receives:** `ap` MUST send back a synthetic `tool_result` for every `tool_use` Claude emitted — the Bedrock/Anthropic API requires a result for every tool use in the conversation turn. The synthetic result has `is_error: true` and a text body containing the cancellation reason. This is the cleanest design: Claude sees the cancellation as a tool error and can decide what to do (retry, apologize, proceed differently).

3. **Injecting the cancellation reason:** The hook's **stdout** is used as the cancellation reason text. If stdout is non-empty, it becomes the `tool_result` content. If stdout is empty (hook exited non-zero silently), `ap` falls back to a default message:
   ```
   Tool call cancelled by pre_tool_call hook (exit code <N>)
   ```

4. **stderr vs stdout:** stderr from the hook is logged to `ap`'s internal log/TUI tool panel for debugging but is NOT sent to Claude. Only stdout feeds into the cancellation reason.

**Example hook (`~/.ap/hooks/pre_tool.sh`):**
```bash
#!/bin/bash
# AP_TOOL_NAME, AP_TOOL_PARAMS are set as env vars
if [ "$AP_TOOL_NAME" = "bash" ]; then
  COMMAND=$(echo "$AP_TOOL_PARAMS" | jq -r '.command')
  if echo "$COMMAND" | grep -q "rm -rf /"; then
    echo "Dangerous command blocked: rm -rf / is not permitted"
    exit 1
  fi
fi
exit 0
```

**Result delivered to Claude when cancelled:**
```json
{
  "type": "tool_result",
  "tool_use_id": "<id>",
  "is_error": true,
  "content": "Dangerous command blocked: rm -rf / is not permitted"
}
```

**Summary of design choices:**
- Exit code = control signal (cancel vs proceed)
- Stdout = cancellation reason (user-controlled message to Claude)
- Stderr = debug channel (not sent to Claude)
- Fallback message if stdout is empty
- Always send a tool_result — never silently drop a tool use

---

**Q2 (2026-03-22):** When Claude emits multiple `tool_use` blocks in a single response (parallel tool calls), should `ap` execute them **concurrently** (tokio tasks in parallel, collecting all results before continuing) or **sequentially** (one at a time, in order)? And does the answer change between TUI mode (where you can show concurrent progress) and non-interactive `-p` mode?

**A2 (2026-03-22):** **Sequential execution, same in both modes** — for v1.

**Why sequential:**

1. **Correctness first.** Coding agent tools frequently have implicit ordering dependencies: a `write` followed by a `bash` (e.g., `cargo build`) that reads what was just written; an `edit` that modifies a file another `read` is about to inspect. Running these in parallel produces non-deterministic results. For a coding agent, correctness beats throughput.

2. **Hooks interact.** If a `pre_tool_call` hook cancels one of N parallel tools, the others are already in flight. Handling partial cancellation is complex. Sequential execution means each hook decision is clean and the conversation state is always consistent.

3. **Simplicity of v1.** The agent loop is already handling streaming responses, async I/O, and TUI updates. Adding `join_all` concurrency introduces race conditions in shared state (e.g., conversation history, the tool-activity panel), which adds risk with minimal user-visible benefit for typical 2–4 tool calls.

4. **Anthropic API compatibility.** Regardless of execution order, the API requires ALL `tool_result` blocks to be batched into a single user turn. Sequential execution naturally satisfies this: collect results one by one, then send them all. Concurrent execution needs an explicit "wait for all" barrier anyway — sequential gives us that for free.

**Same behavior in TUI and `-p` mode:**
- Yes — identical execution semantics in both modes. Behavior must be predictable and reproducible regardless of output target.
- In TUI mode, sequential execution is actually visually clearer: one tool activity indicator lights up at a time, the user sees each tool's output in order, and the right-panel tool log is easy to follow.
- In `-p` mode, sequential output is easier to parse in scripts and logs.

**Architecture path to concurrent (v2+):**
Add a config option `parallel_tools = true` (default `false`). When enabled, the agent loop wraps each tool+hooks execution in a tokio task, uses `futures::future::join_all`, then batches all results. The hook protocol (A1) is compatible: each hook runs as a sub-process per tool independently. The TUI panel already shows a list of tool activities, so it handles N concurrent entries naturally.

**Summary of design choices:**
- v1: sequential execution, both modes
- Order: same order Claude emitted the `tool_use` blocks (deterministic)
- Config hook `pre_tool_call` cancellation during sequential run: cancel that tool, continue running remaining tools in sequence (Claude still gets all results — the cancelled one as an error)
- Config key for future: `[tools] parallel = false` (document it, don't implement it in v1)

---




---

**Q3 answered ✅**

**Q3 (2026-03-22):** For `post_tool_call` hooks — can the hook **modify** the tool result that gets sent to Claude (by writing to stdout), or is it strictly read-only (inspect/log only)? Specifically:
- If the hook writes to stdout, does that replace the `tool_result` content Claude receives?
- Or is `post_tool_call` stdout always ignored (only stderr/logging)?
- What env vars does a `post_tool_call` hook receive? (e.g., `AP_TOOL_NAME`, `AP_TOOL_PARAMS`, `AP_TOOL_RESULT`?)

**A3 (2026-03-22):** `post_tool_call` hooks **CAN modify** the tool result via stdout, using the same stdout-as-content convention established in A1 for `pre_tool_call`.

**Stdout semantics:**
- If stdout is **non-empty** → the hook's stdout **replaces** the `tool_result` content Claude receives.
- If stdout is **empty** → the original tool result is forwarded to Claude unchanged.

This makes `post_tool_call` a transform hook, not just an observer. Real-world use cases that justify this:
1. **Truncation** — a `read` on a 50,000-line file would flood Claude's context; a hook can truncate it and append a note like `[truncated to 500 lines]`
2. **Redaction** — strip secrets, API keys, or PII from bash output before Claude processes it
3. **Normalization** — strip ANSI escape codes from terminal output so Claude receives clean text
4. **Augmentation** — append metadata (e.g., `[file last modified: 2026-01-01]`) to a file read result

**Exit code semantics for post_tool_call:**
- Exit code 0: normal — use stdout as replacement (if non-empty) or pass through original
- Non-zero: **warning only** — `ap` logs the hook failure to the TUI tool panel and the original tool result (NOT the hook's stdout) is forwarded to Claude. `post_tool_call` failures must never silently break the agent loop; they are advisory.

This is different from `pre_tool_call` where non-zero = hard cancel. Rationale: `pre_tool_call` is a gate (preventing execution is meaningful), `post_tool_call` is a transform (if the transform fails, falling back to the real result is the safest behavior).

**Env vars received by `post_tool_call`:**

| Variable | Type | Description |
|---|---|---|
| `AP_TOOL_NAME` | string | Name of the tool that ran (e.g., `bash`, `read`) |
| `AP_TOOL_PARAMS` | JSON string | The parameters passed to the tool |
| `AP_TOOL_RESULT` | JSON string | The tool's result object `{"content": "...", "is_error": false}` |
| `AP_TOOL_IS_ERROR` | `true`/`false` | Convenience flag — whether the tool returned an error |

**For completeness, `pre_tool_call` env vars:**

| Variable | Type | Description |
|---|---|---|
| `AP_TOOL_NAME` | string | Name of the tool about to run |
| `AP_TOOL_PARAMS` | JSON string | The parameters passed to the tool |

`pre_tool_call` has no result env vars because the tool hasn't run yet.

**Example post_tool_call hook (truncation):**
```bash
#!/bin/bash
# Truncate large tool results to protect Claude's context window
if [ "$AP_TOOL_NAME" = "read" ] || [ "$AP_TOOL_NAME" = "bash" ]; then
  CONTENT=$(echo "$AP_TOOL_RESULT" | jq -r '.content')
  LINE_COUNT=$(echo "$CONTENT" | wc -l)
  if [ "$LINE_COUNT" -gt 500 ]; then
    echo "$CONTENT" | head -500
    echo "[Output truncated: showed 500 of $LINE_COUNT lines]"
    # stdout is non-empty → replaces result content sent to Claude
    exit 0
  fi
fi
# stdout empty → original result forwarded unchanged
exit 0
```

**Summary of design choices:**
- post_tool_call is a transform hook (not read-only)
- Non-empty stdout → replaces tool_result content
- Empty stdout → pass-through (original unchanged)
- Non-zero exit → warning, fall back to original (never blocks agent loop)
- Receives: `AP_TOOL_NAME`, `AP_TOOL_PARAMS`, `AP_TOOL_RESULT`, `AP_TOOL_IS_ERROR`
- stderr always → debug log (TUI tool panel), never to Claude



---

**Q4 answered ✅**

**Q4 (2026-03-22):** For `pre_turn` and `post_turn` hooks — can they **modify** the messages going to/from Claude, or are they read-only (observe/log only)?

Specifically:
- **`pre_turn`**: Can stdout from the hook replace or augment the message array sent to Bedrock? If yes, what format does stdout need to be in? If no, what env vars does it receive for inspection?
- **`post_turn`**: Can stdout from the hook modify the assistant response before the agent loop processes it? Or is it observer-only?
- These hooks fire at the whole-conversation level (not per-tool), so what data do they receive — the full message history as JSON, just the latest user message, or something else?

**A4 (2026-03-22):** `pre_turn` and `post_turn` hooks are **read-only observers in v1** — stdout is ignored, and non-zero exit codes are advisory (log a warning to the TUI tool panel but do NOT cancel the turn or alter any messages).

**Why read-only for v1:**

1. **Safety at conversation scope.** `pre_turn` and `post_turn` operate over the full conversation message array and the raw Bedrock response (which contains structured `tool_use` blocks, not just text). Allowing arbitrary replacement of these JSON structures via shell stdout is high-risk: one malformed byte in the replacement payload breaks the entire conversation loop. The reward (modification at turn level) is modest because the tool-call hooks already let hooks intercept and transform the substantive content.

2. **Tool hooks cover the real use cases.** The main reasons a user would want to modify content before Claude sees it (redact secrets, truncate large outputs) are already handled by `post_tool_call` transforms (A3). A `pre_turn` that injects RAG context is a reasonable v2 feature, but not needed for the v1 coding agent scope.

3. **Observer use cases are the common ones.** Logging, metrics, alerting on token budget — all read-only. Making observers read-only keeps the API simple and safe without blocking common workflows.

4. **`pre_turn` cancellation is a special concern.** Cancelling a turn at the `pre_turn` hook would leave Claude without a response to its last message, creating a broken conversation state. Silently dropping a turn is confusing to users. If turn-level cancellation is needed in future, it should be an explicit feature with documented user-visible behavior, not a side effect of hook exit codes.

**Data received by hooks (via temp files for large payloads):**

Because conversation histories can easily exceed Linux's `~128 KB` env var limit, large payloads are written to temp files and the file *path* is injected as an env var. The hook shell-scripts just `cat "$AP_MESSAGES_FILE" | jq .` as needed.

**`pre_turn` env vars:**

| Variable | Type | Description |
|---|---|---|
| `AP_HOOK_TYPE` | string | `"pre_turn"` |
| `AP_TURN_NUMBER` | integer | 1-indexed turn counter for this session |
| `AP_SESSION_ID` | string | Current session identifier |
| `AP_MODEL` | string | Model name (e.g., `us.anthropic.claude-sonnet-4-6`) |
| `AP_MESSAGES_FILE` | path | Path to a temp JSON file: the full messages array about to be sent to Bedrock |

**`post_turn` env vars:**

| Variable | Type | Description |
|---|---|---|
| `AP_HOOK_TYPE` | string | `"post_turn"` |
| `AP_TURN_NUMBER` | integer | 1-indexed turn counter for this session |
| `AP_SESSION_ID` | string | Current session identifier |
| `AP_MODEL` | string | Model name |
| `AP_RESPONSE_FILE` | path | Path to a temp JSON file: the full assistant response content blocks array |
| `AP_HAS_TOOL_USE` | `true`/`false` | Convenience flag — whether the response included tool_use blocks |

**What stdout/exit codes do:**
- Stdout: **ignored** in v1 (any output is discarded)
- Exit 0: normal, continue
- Non-zero: warning logged to TUI tool panel, agent continues unaffected

**Temp file lifecycle:** `ap` creates the temp files before invoking the hook and deletes them after the hook process exits. Hooks must not hold open the files past their own execution.

**Example pre_turn observer (token-budget alert):**
```bash
#!/bin/bash
# Warn if conversation is getting large (observer only)
MESSAGE_COUNT=$(cat "$AP_MESSAGES_FILE" | jq '. | length')
if [ "$MESSAGE_COUNT" -gt 50 ]; then
  echo "[ap hook] WARNING: conversation has $MESSAGE_COUNT messages, context window may be near limit" >&2
fi
exit 0
```

**Example post_turn observer (turn logger):**
```bash
#!/bin/bash
# Log every assistant response to a local file
TIMESTAMP=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo "=== Turn $AP_TURN_NUMBER @ $TIMESTAMP ===" >> ~/.ap/turn-log.jsonl
cat "$AP_RESPONSE_FILE" >> ~/.ap/turn-log.jsonl
echo "" >> ~/.ap/turn-log.jsonl
exit 0
```

**Path to modification (v2+):** Add `pre_turn_transform` and `post_turn_transform` as distinct hook types with explicit JSON-replacement protocol and schema validation. Keeping them separate from the observer hooks means users aren't surprised by accidental modification when they just want to log.

**Summary of design choices:**
- v1: read-only observers (stdout ignored, non-zero = advisory warning only)
- Data delivery: temp file paths in env vars (not env var values) for message arrays/responses
- `pre_turn` receives full messages array file; `post_turn` receives response content blocks file
- Temp files cleaned up after hook exits
- Modification at turn level is deferred to v2 `pre_turn_transform` / `post_turn_transform` hook types


---

**Q5 (2026-03-22) — post design.rejected:** For the `edit` tool, when `old_text` appears **more than once** in the target file, what should the tool do?

Options:
1. **Replace the first occurrence only** (predictable, matches how most editors work)
2. **Replace all occurrences** (may produce unexpected results if the user only intended to change one)
3. **Return an error** (refuse to proceed if old_text is ambiguous; force the caller to provide a more unique string)

This affects both what Claude receives as feedback and how the tool behaves when called by extensions.

**A5 (2026-03-22):** **Option 3 — return an error.** Reasoning:

- An AI coding agent must be precise. Silent replacement of all occurrences risks subtle bugs (e.g., renaming a variable in one scope when the LLM only intended to change one call site).
- Returning an error forces the LLM to provide more context in `old_text` to make the match unique — this is the correct behavior (the LLM should add surrounding lines until the string is unambiguous).
- This matches how pi's edit tool and Claude's native tool_use work in practice. It is the established convention in AI coding agents.
- Replacing only the first occurrence is also problematic: the LLM cannot know which occurrence it got, leading to silent mistakes.

**Design impact:** `edit` tool returns `ToolResult { is_error: true, content: "old_text matches N occurrences (must be unique)" }` when N > 1. The error message includes the count so the LLM knows how to fix its call.

