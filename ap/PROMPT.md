# Session Management UX — Implementation Prompt

## Vision

Every `ap` invocation creates a named, persisted session. There are no throwaway
runs. The first user message auto-generates a human-readable slug
(`refactor-auth-module-2026-03-22`); `--session <name>` gives an explicit name.
Sessions survive restarts, can be listed, resumed, or forked. The TUI gains a
session browser overlay (`s` key) so the user never has to leave the terminal to
manage history.

The existing `--session <id>` opt-in flag is **replaced**: persistence is always
on, and `--session` now means "name this session" rather than "enable saving".

---

## Technical Context

### File layout (relevant modules)

```
src/
  main.rs              — CLI entry-point: Args, run_headless(), run_tui()
  session/
    mod.rs             — Session struct + format_unix_as_iso8601()
    store.rs           — SessionStore: save/load Session + Conversation
  tui/
    mod.rs             — TuiApp, AppMode, handle_submit(), handle_ui_event()
    events.rs          — handle_key_event() → Action
    ui.rs              — render(), chat_entries_to_lines()
  config.rs            — AppConfig, ProviderConfig, …
  types.rs             — Conversation, TurnEvent, Middleware
```

### Key existing types (do not break these signatures)

```rust
// types.rs
pub struct Conversation {
    pub id: String,          // used as session name/slug
    pub model: String,
    pub messages: Vec<Message>,
    pub config: AppConfig,
    #[serde(skip)]
    pub system_prompt: Option<String>,
}

// session/mod.rs
pub struct Session {
    pub id: String,
    pub created_at: String,  // ISO 8601 UTC
    pub model: String,
    pub messages: Vec<Message>,
}

// session/store.rs
pub struct SessionStore { pub base: PathBuf }
impl SessionStore {
    pub fn new() -> Result<Self>
    pub fn with_base(base: PathBuf) -> Self
    pub fn save(&self, session: &Session) -> Result<()>
    pub fn load(&self, id: &str) -> Result<Session>
    pub fn save_conversation(&self, conv: &Conversation) -> Result<()>
    pub fn load_conversation(&self, id: &str) -> Result<Conversation>
}
```

### CLI args (current, to be extended)

```rust
struct Args {
    prompt:        Option<String>,   // -p / --prompt
    session:       Option<String>,   // -s / --session  ← rename semantics
    context_limit: Option<u32>,
}
```

---

## Implementation Steps

Each step must leave `cargo build` and `cargo test` green before moving on.

---

### Step 1 — `SessionStore::list()` and `SessionMeta`

**Add to `src/session/store.rs`:**

```rust
/// Lightweight metadata for the session browser and `ap sessions` listing.
/// Derived from the on-disk JSON without loading full message bodies.
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: String,
    pub created_at: String,   // ISO 8601 string from Session.created_at
    pub model: String,
    pub turn_count: usize,    // number of messages / 2  (user+assistant pairs)
    pub last_snippet: String, // last assistant text, truncated to 80 chars
}

impl SessionStore {
    /// Return metadata for every `*.json` file in `self.base`, sorted by
    /// `created_at` descending (most-recent first).
    /// Files that fail to parse are silently skipped.
    pub fn list(&self) -> Vec<SessionMeta> { … }
}
```

**Rules:**
- Parse each file as a `Conversation` (not `Session`) since that is what
  `save_conversation` writes.
- `turn_count` = `conv.messages.len() / 2`.
- `last_snippet`: scan `conv.messages` in reverse for the first assistant
  `MessageContent::Text { text }`, take the first line of `text`, truncate to
  80 chars (append `…` if truncated).  Use `""` if none found.
- Sort by `conv.created_at` descending — add `#[serde(default)] pub created_at:
  String` to `Conversation` (default `""`).
- Files that `serde_json::from_str` fails on are silently skipped.

**New unit tests in `store.rs`:**
- `list_empty_dir_returns_empty` — store pointed at non-existent dir → `[]`.
- `list_returns_metadata_for_saved_conversations` — save two `Conversation`s
  (with messages), call `list()`, assert both present, correct `turn_count`.
- `list_skips_malformed_files` — write `bad.json` with `{}` content → not in
  result, valid file still appears.
- `list_sorted_most_recent_first` — two conversations with different
  `created_at` strings, assert order.

---

### Step 2 — Slug generation (`src/session/slug.rs`)

**Create `src/session/slug.rs`:**

```rust
/// Generate a short human-readable session slug from the first user message.
///
/// Algorithm:
///   1. Lowercase the input.
///   2. Keep only ASCII letters, digits, and spaces; strip everything else.
///   3. Split on whitespace, take first 5 words.
///   4. Join with `-`.
///   5. Append `-YYYY-MM-DD` using today's UTC date (from `SystemTime`).
///   6. If the result is empty after step 4, use `"session-YYYY-MM-DD"`.
///
/// The returned slug contains only `[a-z0-9-]`.
pub fn slug_from_message(message: &str) -> String { … }

/// Generate a slug using an explicit date suffix (YYYY-MM-DD) instead of today.
/// Used by tests to produce deterministic output.
pub fn slug_from_message_with_date(message: &str, date: &str) -> String { … }
```

**Expose from `src/session/mod.rs`:**
```rust
pub mod slug;
pub use slug::slug_from_message;
```

**Unit tests in `slug.rs`:**
- `slug_from_normal_message` — `"Refactor the auth module"` with date
  `"2026-03-22"` → `"refactor-the-auth-module-2026-03-22"`.
- `slug_strips_punctuation` — `"Fix bug #42: don't break it!"` → starts with
  `"fix-bug-42-dont-break-it"` (punctuation removed, only letters/digits/spaces
  kept before joining).
- `slug_truncates_to_five_words` — message with 10 words → slug has exactly
  5 content words before the date suffix.
- `slug_empty_message_uses_fallback` — `""` → starts with `"session-"`.
- `slug_only_punctuation_uses_fallback` — `"!!! ???"` → starts with `"session-"`.
- `slug_is_lowercase` — any mixed-case input → result is all lowercase.

---

### Step 3 — Always-on session persistence in `run_headless()`

**Modify `src/main.rs`:**

Change `run_headless` so that:

1. A `SessionStore` is **always** created (not only when `--session` is given).
2. The session name is resolved as:
   - `--session <name>` → use `<name>` directly.
   - No flag → generate `slug_from_message(prompt)`.
3. `Conversation::new` is called with the resolved name as its `id`.
4. If a file `~/.ap/sessions/<name>.json` already exists, load it with
   `store.load_conversation(&name)` and resume it (append the new user message
   on top of the loaded history).
5. After a successful turn, **always** call `store.save_conversation(&updated_conv)`.
6. Print to stderr on startup:  
   `ap: session: <name>` (new session)  
   `ap: resuming session <name> (<N> messages)` (loaded session)

**Update `Args`:**

```rust
struct Args {
    prompt:        Option<String>,
    /// Give this session an explicit name (otherwise auto-generated from prompt)
    #[arg(short = 's', long = "session")]
    session:       Option<String>,
    context_limit: Option<u32>,
    /// Resume the most recent session (or a named one with --session)
    #[arg(long)]
    resume:        bool,
    /// Fork a past session into a new named one
    #[arg(long)]
    fork:          Option<String>,
}
```

When `--resume` is set (and no explicit `--session`):
- Load `store.list()`, take the first entry (most recent).
- Use that session's `id` as the name, load the conversation, and resume it.

When `--fork <name>` is set:
- Load `store.load_conversation(&name)`.
- Generate a new slug: `"fork-<name>-YYYY-MM-DD"`.
- Create a new `Conversation` with the forked messages but the new id.
- Save the fork immediately (even before the first turn).

**No new tests required in this step** — the integration is covered by Step 6's
acceptance criteria and the compile check.

---

### Step 4 — `ap sessions` subcommand

**Add a subcommand to `Args`:**

```rust
#[derive(Parser, Debug)]
#[command(name = "ap", …)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    // … existing fields …
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// List all saved sessions
    Sessions,
}
```

When `args.command == Some(Commands::Sessions)`:

```
ap sessions
```

Prints a formatted table to stdout:

```
NAME                                  DATE        TURNS  LAST MESSAGE
refactor-auth-module-2026-03-22       2026-03-22      4  Sure, I'll refactor…
session-2026-03-21                    2026-03-21      1  Here is the output…
```

Rules:
- Column widths: NAME=40, DATE=12, TURNS=6, LAST MESSAGE=remainder.
- If no sessions exist: print `No sessions found.` and exit 0.
- Uses `SessionStore::list()` from Step 1.
- Exits the process after printing (does not enter TUI).

**Unit test (`main.rs` or a new `src/session/format.rs`):**
- `format_sessions_table_empty` — `list = []` → `"No sessions found.\n"`.
- `format_sessions_table_one_row` — one `SessionMeta` → correct columns.

Extract the formatting logic into a pure function:

```rust
pub fn format_sessions_table(sessions: &[SessionMeta]) -> String { … }
```

---

### Step 5 — Always-on session persistence in `run_tui()`

**Modify `run_tui()` and `TuiApp`:**

1. `run_tui()` **always** creates a `SessionStore`.
2. The initial conversation `id` is set to a placeholder `"pending-<uuid>"`.  
   After the **first** user message is submitted, replace the id with
   `slug_from_message(&first_user_message)`.  
   (A `--session <name>` flag skips slug generation and uses the given name
   immediately.)
3. After every successful turn (`TurnEvent::TurnEnd`), save the conversation via
   `store.save_conversation(&conv)`.
4. `TuiApp` gains a field:

```rust
pub store: Arc<SessionStore>,
pub session_name_locked: bool,  // false until the name is finalised
```

5. `handle_submit()` (inside the spawned task): after turn succeeds, send a new
   `TurnEvent` variant `SessionSaved { name: String }` so the status bar can
   show the session name.

**Add to `TurnEvent`:**
```rust
TurnEvent::SessionSaved { name: String },
```

Handle in `handle_ui_event`:
- Store the session name in `TuiApp::session_name: String` (default `""`).
- The status bar already renders `app.model_name`; add `app.session_name` next
  to it: `ap │ <session> │ <model> │ …`.

**Update `TuiApp::new` signature:**

```rust
pub fn new(
    conv: Arc<tokio::sync::Mutex<Conversation>>,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<Middleware>,
    model_name: String,
    context_limit: Option<u32>,
    store: Arc<SessionStore>,       // ← new
    initial_session_name: String,   // ← new (may be "pending-<uuid>" or explicit)
) -> Result<Self>
```

Update the headless test constructor to supply stub values:
```rust
#[cfg(test)]
pub fn headless() -> Self { … }  // pass a tempdir-backed store
```

---

### Step 6 — TUI session browser overlay

**Add `AppMode::SessionBrowser` to `tui/mod.rs`:**

```rust
pub enum AppMode {
    Normal,
    Insert,
    SessionBrowser,
}
```

**Add to `TuiApp`:**

```rust
pub show_session_browser: bool,
pub session_list: Vec<SessionMeta>,
pub session_cursor: usize,         // currently highlighted row
```

**Key bindings (add to `events.rs`):**

| Key | Mode | Action |
|-----|------|--------|
| `s` | Normal | `OpenSessionBrowser` |
| `Esc` | SessionBrowser | `CloseSessionBrowser` |
| `j` / Down | SessionBrowser | `SessionCursorDown` |
| `k` / Up | SessionBrowser | `SessionCursorUp` |
| `Enter` | SessionBrowser | `SessionResume` |
| `f` | SessionBrowser | `SessionFork` |

Add corresponding `Action` variants:

```rust
pub enum Action {
    // … existing …
    OpenSessionBrowser,
    CloseSessionBrowser,
    SessionCursorDown,
    SessionCursorUp,
    SessionResume,
    SessionFork,
}
```

**Overlay rendering in `ui.rs`:**

Add `render_session_browser(frame, app, area)`:

- Centred modal, 80% width × 70% height.
- Left pane (60%): scrollable list of session names + dates.
  Highlighted row uses `theme.accent` background.
- Right pane (40%): preview — `last_snippet` of highlighted session,
  turn count, created date.
- Footer: `Enter resume  f fork  Esc close`.

**`handle_session_resume(&mut self)`:**
- Load `self.session_list[self.session_cursor]` via `self.store`.
- Replace `conv` with the loaded conversation.
- Clear `chat_history`, repopulate from `conv.messages` (user messages →
  `ChatEntry::User`, assistant messages → `ChatEntry::AssistantDone`).
- Lock the session name.
- Close the overlay.

**`handle_session_fork(&mut self)`:**
- Load the selected session.
- Generate fork name: `slug_from_message_with_date("fork-<original-id>", today)`.
- Create a new `Conversation` with forked messages, new id.
- Save immediately via `self.store`.
- Replace active `conv` and rebuild `chat_history`.
- Close the overlay.

**Unit tests (headless, no terminal):**

- `session_browser_opens_on_s_key` — Normal mode, `s` key → `show_session_browser = true`, mode = `SessionBrowser`.
- `session_browser_closes_on_esc` — SessionBrowser mode, Esc → `show_session_browser = false`, mode = `Normal`.
- `session_cursor_moves_down` — list with 3 entries, press `j` → `session_cursor = 1`.
- `session_cursor_clamps_at_bottom` — cursor at last entry, press `j` → stays.
- `session_cursor_moves_up` — cursor at 1, press `k` → `session_cursor = 0`.
- `session_cursor_clamps_at_top` — cursor at 0, press `k` → stays.

---

### Step 7 — `--resume` and `--fork` in TUI mode

Extend `run_tui()` to handle the new CLI flags:

- `--resume` (no name): call `store.list()`, load the most recent session,
  initialise `TuiApp` with that conversation and locked session name.
- `--resume` + `--session <name>`: load exactly that session (or fuzzy-match:
  iterate `store.list()`, find first whose `id` contains `<name>` as substring).
- `--fork <name>`: same fork logic as the TUI `f` key — load, generate new name,
  save, initialise TuiApp with forked conv.

For fuzzy match: a pure function in `src/session/mod.rs`:

```rust
/// Return the first session whose id contains `query` as a case-insensitive
/// substring, from a pre-sorted (most-recent-first) list.
pub fn fuzzy_find<'a>(sessions: &'a [SessionMeta], query: &str) -> Option<&'a SessionMeta> { … }
```

**Unit tests:**
- `fuzzy_find_exact_match` — query == id → found.
- `fuzzy_find_substring_match` — query `"auth"` matches `"refactor-auth-module-2026-03-22"`.
- `fuzzy_find_case_insensitive` — query `"AUTH"` matches lowercase id.
- `fuzzy_find_no_match_returns_none`.
- `fuzzy_find_returns_first_when_multiple_match`.

---

## Acceptance Criteria

All of the following must be true for `LOOP_COMPLETE`:

1. **`cargo build` exits 0** with no warnings (the project has `#![deny(…)]`
   and `[lints.clippy] unwrap_used = "deny"` etc. — all must pass).

2. **`cargo test` exits 0** — all existing tests pass, all new tests listed
   above pass.

3. **`SessionStore::list()`** exists, returns `Vec<SessionMeta>`, sorted
   most-recent-first, skips malformed files.

4. **`slug_from_message("Refactor the auth module")`** returns a string
   matching `^refactor-the-auth-module-\d{4}-\d{2}-\d{2}$`.

5. **Always-on headless persistence:** running `ap -p "hello"` (no `--session`)
   creates a file in `~/.ap/sessions/` whose name matches the slug of `"hello"`.
   (Verified by the Step 3 logic; tested indirectly via unit tests on
   `slug_from_message` and `SessionStore`.)

6. **`ap sessions` subcommand** compiles and, when `~/.ap/sessions/` is empty,
   prints `No sessions found.`

7. **`TurnEvent::SessionSaved`** variant exists and is handled in
   `handle_ui_event` without `unreachable!()` or `panic!()`.

8. **`AppMode::SessionBrowser`** exists; `s` in Normal mode opens the overlay;
   `Esc` closes it.

9. **`TuiApp::new`** accepts `store: Arc<SessionStore>` and
   `initial_session_name: String` parameters.

10. **`fuzzy_find`** exists in `src/session/mod.rs` and passes all unit tests.

11. **No regressions** — all tests that existed before this work still pass.

---

## Coding Standards

- Functional-first: pure functions, immutable-friendly types, iterator chains.
- No `.unwrap()` or `.expect()` in non-test code — use `?` or `unwrap_or_else`.
- No `panic!()` in non-test code.
- Every new public function has a doc comment.
- New modules must be declared in `lib.rs` or their parent `mod.rs`.
- Keep `Conversation.id` as the canonical session name/slug — do not add a
  separate `name` field.
- `Session` (legacy) and `Conversation` (canonical) coexist; new code uses
  `Conversation`.

---

## Output

Output `LOOP_COMPLETE` when all acceptance criteria are met and `cargo build && cargo test` exits 0 with no warnings or test failures.
