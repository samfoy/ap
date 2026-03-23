# PROMPT.md — Session persistence in `--prompt` mode

## Vision

`ap --prompt "..."` is the headless/scriptable face of the agent. It currently
discards the conversation after every run. The goal is to make it a first-class
citizen of the session system: every headless run creates a named session file in
`~/.ap/sessions/`, exactly as the interactive TUI does (or will do).

After this work:

- `ap --prompt "fix the tests"` creates `~/.ap/sessions/prompt-fix-the-tests-2026-03-22.json`
- `ap --prompt "fix the tests" --session my-fix` creates `~/.ap/sessions/my-fix.json`
- `ap --prompt "continue" --session my-fix` resumes that session
- Stdout still receives the streamed assistant response (no behavioural regression)
- The session file is a standard `Conversation` JSON (same schema as TUI sessions)

---

## Technical context

### Relevant files

| File | Role |
|---|---|
| `src/main.rs` | Entry point; `run_headless()` owns all `--prompt` logic |
| `src/session/mod.rs` | `Session` value type (not used by headless currently) |
| `src/session/store.rs` | `SessionStore` — `save_conversation` / `load_conversation` |
| `src/types.rs` | `Conversation` — immutable value, `#[derive(Serialize, Deserialize)]` |
| `tests/noninteractive.rs` | Existing integration tests for headless path |

### Key types (do not change their public signatures)

```rust
// src/types.rs
pub struct Conversation {
    pub id: String,
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub config: AppConfig,
    #[serde(skip)]
    pub system_prompt: Option<String>,
}

impl Conversation {
    pub fn new(id: impl Into<String>, model: impl Into<String>, config: AppConfig) -> Self;
    pub fn with_user_message(self, content: impl Into<String>) -> Self;
    pub fn with_system_prompt(self, prompt: impl Into<String>) -> Self;
    pub fn with_messages(self, messages: Vec<Message>) -> Self;
}

// src/session/store.rs
pub struct SessionStore { pub base: PathBuf }
impl SessionStore {
    pub fn new() -> Result<Self>;
    pub fn with_base(base: PathBuf) -> Self;
    pub fn save_conversation(&self, conv: &Conversation) -> Result<()>;
    pub fn load_conversation(&self, id: &str) -> Result<Conversation>;
}
```

### Current `run_headless` behaviour (baseline)

```rust
async fn run_headless(
    config: AppConfig,
    session_id: Option<String>,   // Some only when --session passed
    prompt: &str,
) -> anyhow::Result<()>
```

Session is only saved when `--session` is explicitly supplied. Without it, the
conversation is `"ephemeral"` and is discarded.

---

## Slug generation

A **session slug** is a URL-safe, human-readable identifier derived from the
prompt plus the current UTC date:

```
prompt-<words>-<YYYY-MM-DD>
```

Rules (pure function, no I/O):

1. Take the first **6 words** of the prompt (split on whitespace).
2. Lowercase every word.
3. Replace every run of non-alphanumeric characters in each word with `-`.
4. Strip leading/trailing `-` from each word; drop empty words.
5. Join words with `-`, prefix `prompt-`, suffix `-<YYYY-MM-DD>`.
6. Truncate the entire string to **60 characters** maximum.

Examples:

| Prompt | Expected slug (2026-03-22) |
|---|---|
| `"hello"` | `"prompt-hello-2026-03-22"` |
| `"Read the backlog and fix item 3"` | `"prompt-read-the-backlog-and-fix-2026-03-22"` |
| `"Fix tests!!!"` | `"prompt-fix-tests-2026-03-22"` |
| `"  leading spaces  "` | `"prompt-leading-spaces-2026-03-22"` |

The date must come from the system clock (UTC). Use the existing
`format_unix_as_iso8601` logic already in `src/session/mod.rs` for the date
part (or replicate the Julian Day approach there — no `chrono` dependency).

---

## Implementation steps

Each step must leave the project in a **compilable, test-passing** state.
Run `cargo test --lib` after each step to verify.

---

### Step 1 — Pure `prompt_slug` function in `src/session/mod.rs`

Add a public function:

```rust
/// Derive a session slug from a prompt string and a Unix timestamp (seconds).
///
/// Format: `prompt-<up-to-6-slugified-words>-<YYYY-MM-DD>`
/// Maximum length: 60 characters.
pub fn prompt_slug(prompt: &str, unix_secs: u64) -> String
```

- Lives in `src/session/mod.rs` (next to the existing `format_unix_as_iso8601`
  helper which it should call for the date part).
- **Pure function** — no `SystemTime` calls inside; the caller supplies
  `unix_secs` so tests can be deterministic.
- Exposed as `pub` so `main.rs` can call `ap::session::prompt_slug(...)`.

Add unit tests in the same file under `#[cfg(test)]`:

```rust
// unix timestamp for 2026-03-22T00:00:00Z = 1_742_601_600
const T: u64 = 1_742_601_600;

assert_eq!(prompt_slug("hello", T), "prompt-hello-2026-03-22");
assert_eq!(
    prompt_slug("Read the backlog and fix item 3", T),
    "prompt-read-the-backlog-and-fix-2026-03-22"
);
assert_eq!(prompt_slug("Fix tests!!!", T), "prompt-fix-tests-2026-03-22");
assert_eq!(prompt_slug("  leading spaces  ", T), "prompt-leading-spaces-2026-03-22");
// Truncation: slug must never exceed 60 chars
let long = "a".repeat(200);
assert!(prompt_slug(&long, T).len() <= 60);
```

**Compile check:** `cargo test --lib -- session`

---

### Step 2 — Auto-generate session name in `run_headless` when `--session` is absent

Modify `run_headless` in `src/main.rs`:

**Before** (current logic):
```rust
let store: Option<SessionStore> = session_id.as_ref().map(|_| { ... });

let conv: Conversation = match (&session_id, &store) {
    (Some(id), Some(s)) => /* load or create with explicit id */,
    _ => Conversation::new("ephemeral", ...),   // ← discarded
};
```

**After:**
```rust
// Resolve session name: explicit --session flag, or auto-generate from prompt
let resolved_session_id: String = session_id
    .clone()
    .unwrap_or_else(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        ap::session::prompt_slug(prompt, now)
    });

// Always create a SessionStore (needed for both explicit and auto sessions)
let store = SessionStore::new().unwrap_or_else(|e| {
    eprintln!("ap: warning: could not determine session dir: {e}");
    SessionStore::with_base(std::path::PathBuf::from(".ap/sessions"))
});

// Load existing session or create fresh Conversation
let conv: Conversation = match store.load_conversation(&resolved_session_id) {
    Ok(c) => {
        eprintln!(
            "ap: resuming session {} ({} messages)",
            resolved_session_id,
            c.messages.len()
        );
        c
    }
    Err(_) => Conversation::new(
        resolved_session_id.clone(),
        config.provider.model.clone(),
        config.clone(),
    ),
};
```

Remove the now-dead `Option<SessionStore>` pattern.  
At the end of `run_headless`, always save (not only when `session_id.is_some()`):

```rust
// Save session after every successful headless run
if exit_code == 0 {
    if let Err(e) = store.save_conversation(&updated_conv) {
        eprintln!("ap: warning: could not save session: {e}");
    } else {
        eprintln!("ap: session saved: {}", resolved_session_id);
    }
}
```

**Compile check:** `cargo build`

---

### Step 3 — Integration test: session file is created by `ap --prompt`

Add a new test in `tests/noninteractive.rs` (or a new file
`tests/session_persistence.rs`).

The test must:

1. Use `MockProvider` (copy the pattern from `tests/noninteractive.rs`).
2. Invoke `turn()` directly (same as existing headless tests — no subprocess).
3. Exercise the session-save logic by calling `SessionStore::save_conversation`
   on the resulting `Conversation` and asserting the file exists.
4. Verify the slug format when no explicit session name is given.

```rust
#[test]
fn prompt_slug_format_matches_spec() {
    use ap::session::prompt_slug;
    // 2026-03-22T00:00:00Z
    let t = 1_742_601_600u64;
    let slug = prompt_slug("hello world", t);
    assert!(slug.starts_with("prompt-hello-world-"), "got: {slug}");
    assert!(slug.ends_with("2026-03-22"), "got: {slug}");
}

#[tokio::test]
async fn headless_turn_saves_session_to_store() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let store = ap::session::store::SessionStore::with_base(tmp.path().to_path_buf());

    let provider = Arc::new(MockProvider::new(vec![vec![
        StreamEvent::TextDelta("hi".to_string()),
        StreamEvent::TurnEnd {
            stop_reason: "end_turn".to_string(),
            input_tokens: 5,
            output_tokens: 2,
        },
    ]]));

    let session_id = "prompt-hello-2026-03-22";
    let conv = Conversation::new(session_id, "claude-3", AppConfig::default())
        .with_user_message("hello");
    let tools = ToolRegistry::with_defaults();
    let middleware = Middleware::new();

    // Act
    let (updated_conv, _events) = turn(conv, provider.as_ref(), &tools, &middleware)
        .await
        .expect("turn failed");

    store.save_conversation(&updated_conv).expect("save failed");

    // Assert: file exists at expected path
    let expected_path = tmp.path().join(format!("{session_id}.json"));
    assert!(
        expected_path.exists(),
        "session file should exist at {}", expected_path.display()
    );

    // Assert: file is valid JSON with correct id
    let loaded = store.load_conversation(session_id).expect("load failed");
    assert_eq!(loaded.id, session_id);
    assert!(!loaded.messages.is_empty(), "should have at least one message");
}
```

**Compile check:** `cargo test`

---

### Step 4 — Update `--session` flag description and help text in `src/main.rs`

The `--session` clap argument description should mention that it works in both
`--prompt` and interactive modes:

```rust
/// Name or resume a session. In --prompt mode, defaults to a slug derived
/// from the prompt text if not specified.
#[arg(short = 's', long = "session")]
session: Option<String>,
```

No logic change — documentation only.

**Compile check:** `cargo build`

---

### Step 5 — Backlog housekeeping

Mark item 0 complete in `BACKLOG.md` (or wherever the project backlog lives).
The line should change from:

```
0. [ ] **Session persistence in --prompt mode** — ...
```

to:

```
0. [x] **Session persistence in --prompt mode** — ...
```

**Compile check:** `cargo test` (full suite must be green)

---

## Acceptance criteria

All of the following must be true before the loop is considered complete:

| # | Criterion |
|---|---|
| AC-1 | `cargo build` succeeds with zero errors and zero new warnings |
| AC-2 | `cargo test` passes (all existing tests + new tests green) |
| AC-3 | `src/session/mod.rs` exports `pub fn prompt_slug(prompt: &str, unix_secs: u64) -> String` |
| AC-4 | `prompt_slug("hello", 1_742_601_600)` returns `"prompt-hello-2026-03-22"` |
| AC-5 | `prompt_slug` output never exceeds 60 characters for any input |
| AC-6 | `run_headless` always creates a `SessionStore` (not only when `--session` is given) |
| AC-7 | When `--session` is absent, `run_headless` calls `prompt_slug` to derive the id |
| AC-8 | When `--session <name>` is supplied, `run_headless` uses that name (existing behaviour preserved) |
| AC-9 | After a successful turn, `run_headless` calls `store.save_conversation(&updated_conv)` unconditionally (guarded only by `exit_code == 0`) |
| AC-10 | Integration test `headless_turn_saves_session_to_store` passes |
| AC-11 | Stdout still receives streamed text (no regression to `route_headless_events`) |
| AC-12 | Item 0 in the backlog is marked `[x]` |

---

## Constraints

- **No new dependencies** — all required types (`dirs`, `uuid`, `serde_json`,
  `anyhow`) are already in `Cargo.toml`.
- **No `chrono`** — use the existing `format_unix_as_iso8601` helper or replicate
  the same Julian Day arithmetic.
- **Functional-first** — `prompt_slug` must be a pure function; all I/O stays in
  `run_headless`.
- **`#[deny(clippy::unwrap_used)]` and `#[deny(unsafe_code)]`** remain in force;
  use `unwrap_or_default()` / `?` / `unwrap_or_else` as appropriate.
- Do not change the public API of `Conversation`, `SessionStore`, or `turn()`.
- Do not modify `tests/noninteractive.rs` existing test cases — only add new ones.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project
builds clean.
