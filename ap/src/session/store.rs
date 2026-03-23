//! `SessionStore` — save and load named sessions in JSONL-in-subdir format.
//!
//! Each session lives in `~/.ap/sessions/<name>/`:
//! - `conversation.jsonl` — one `Message` JSON per line
//! - `meta.json`          — `{ name, created_at, model }`
//!
//! Tests inject a `PathBuf` via [`SessionStore::with_base`] so the real code
//! paths are exercised without touching `$HOME`.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::provider::Message;
use crate::types::Conversation;

// ─── Word lists for generate_name() ──────────────────────────────────────────

const ADJECTIVES: &[&str] = &[
    "amber", "ancient", "arctic", "azure", "bold", "brave", "bright", "calm",
    "clear", "cool", "crisp", "crystal", "dark", "deep", "deft", "distant",
    "fair", "fast", "fierce", "free", "fresh", "frosted", "golden", "grand",
    "great", "green", "grey", "high", "keen", "kind", "large", "late", "lean",
    "light", "long", "loud", "low", "mild", "misty", "neat", "nimble", "noble",
    "open", "plain", "pure", "quick", "quiet", "rare", "rich", "rough", "sharp",
    "silent", "slim", "slow", "small", "soft", "still", "swift", "tall", "thin",
    "true", "warm", "wide", "wild", "wise", "young",
];

const NOUNS: &[&str] = &[
    "ash", "bay", "birch", "brook", "cave", "cedar", "cliff", "cloud", "creek",
    "dawn", "dell", "dune", "dusk", "elm", "fern", "field", "fire", "fjord",
    "ford", "glade", "glen", "grove", "hill", "isle", "lake", "leaf", "maple",
    "mist", "moor", "moss", "oak", "peak", "pine", "pool", "rain", "reef",
    "ridge", "rift", "rise", "river", "rock", "sage", "sand", "sea", "shade",
    "shore", "sky", "snow", "star", "stem", "stone", "storm", "stream", "tide",
    "tree", "vale", "vine", "wave", "wind",
];

// ─── On-disk meta.json representation ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MetaOnDisk {
    name: String,
    created_at: String,
    model: String,
}

// ─── Public SessionMeta struct ────────────────────────────────────────────────

/// Public metadata about a saved session (returned by `list()`).
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub name: String,
    pub created_at: String,
    pub model: String,
    pub message_count: usize,
}

// ─── SessionStore ─────────────────────────────────────────────────────────────

/// Saves and loads named sessions from a directory on disk.
///
/// Use [`SessionStore::new`] for the default `~/.ap/sessions/` location, or
/// [`SessionStore::with_base`] to point at an arbitrary directory (e.g. a
/// `tempdir` in tests).
pub struct SessionStore {
    /// Root directory where per-session subdirectories are stored.
    pub base: PathBuf,
}

impl SessionStore {
    /// Create a store backed by `~/.ap/sessions/`.
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(Self {
            base: home.join(".ap").join("sessions"),
        })
    }

    /// Create a store backed by an arbitrary directory (useful in tests).
    pub fn with_base(base: PathBuf) -> Self {
        Self { base }
    }

    /// Returns the session directory path: `<base>/<name>/`.
    fn session_dir(&self, name: &str) -> PathBuf {
        self.base.join(name)
    }

    /// Save a conversation to `<base>/<name>/conversation.jsonl` + `meta.json`.
    ///
    /// - The JSONL file is always rewritten with all messages (not append-only).
    /// - `meta.json` is created once on the first save and never overwritten,
    ///   preserving the original `created_at` timestamp.
    pub fn save(&self, name: &str, conversation: &Conversation) -> Result<()> {
        let dir = self.session_dir(name);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create session dir: {}", dir.display()))?;

        // Write conversation.jsonl (full rewrite)
        let jsonl_path = dir.join("conversation.jsonl");
        let mut lines = Vec::with_capacity(conversation.messages.len());
        for msg in &conversation.messages {
            let line = serde_json::to_string(msg)
                .with_context(|| "failed to serialize message")?;
            lines.push(line);
        }
        // Each line terminated by newline; empty file if no messages
        let content = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };
        std::fs::write(&jsonl_path, content)
            .with_context(|| format!("failed to write conversation.jsonl: {}", jsonl_path.display()))?;

        // Write meta.json only on first save (idempotent — preserves created_at)
        let meta_path = dir.join("meta.json");
        if !meta_path.exists() {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let meta = MetaOnDisk {
                name: name.to_string(),
                created_at: super::format_unix_as_iso8601(secs),
                model: conversation.model.clone(),
            };
            let meta_json = serde_json::to_string_pretty(&meta)
                .context("failed to serialize meta.json")?;
            std::fs::write(&meta_path, meta_json)
                .with_context(|| format!("failed to write meta.json: {}", meta_path.display()))?;
        }

        Ok(())
    }

    /// Load messages from `<base>/<name>/conversation.jsonl`.
    ///
    /// Returns `Err` if the session directory or JSONL file does not exist.
    pub fn load(&self, name: &str) -> Result<Vec<Message>> {
        let jsonl_path = self.session_dir(name).join("conversation.jsonl");
        if !jsonl_path.exists() {
            bail!("session not found: {name}");
        }
        let contents = std::fs::read_to_string(&jsonl_path)
            .with_context(|| format!("failed to read conversation.jsonl for session: {name}"))?;
        let mut messages = Vec::new();
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let msg: Message = serde_json::from_str(trimmed)
                .with_context(|| format!("failed to parse message in session '{name}': {trimmed}"))?;
            messages.push(msg);
        }
        Ok(messages)
    }

    /// List all sessions in `<base>/`.
    ///
    /// Returns an empty `Vec` if the base directory does not exist. Skips
    /// any session directories that cannot be parsed (logs a warning to stderr).
    /// Results are sorted by `created_at` descending (newest first).
    pub fn list(&self) -> Result<Vec<SessionMeta>> {
        if !self.base.exists() {
            return Ok(Vec::new());
        }
        let read_dir = std::fs::read_dir(&self.base)
            .with_context(|| format!("failed to read sessions dir: {}", self.base.display()))?;

        let mut sessions = Vec::new();
        for entry in read_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("Warning: failed to read session dir entry: {e}");
                    continue;
                }
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let meta_path = path.join("meta.json");
            let meta: MetaOnDisk = match std::fs::read_to_string(&meta_path) {
                Ok(s) => match serde_json::from_str(&s) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!(
                            "Warning: failed to parse meta.json at {}: {e}",
                            meta_path.display()
                        );
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!(
                        "Warning: failed to read meta.json at {}: {e}",
                        meta_path.display()
                    );
                    continue;
                }
            };
            // Count lines in conversation.jsonl for message_count
            let jsonl_path = path.join("conversation.jsonl");
            let message_count = std::fs::read_to_string(&jsonl_path)
                .map_or(0, |s| s.lines().filter(|l| !l.trim().is_empty()).count());
            sessions.push(SessionMeta {
                name: meta.name,
                created_at: meta.created_at,
                model: meta.model,
                message_count,
            });
        }

        // Sort by created_at descending (ISO 8601 sorts lexicographically)
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(sessions)
    }

    /// Generate a random `adjective-noun` session name.
    ///
    /// Uses UUID v4 bytes for randomness — no `rand` crate required.
    pub fn generate_name() -> String {
        let bytes = uuid::Uuid::new_v4();
        let b = bytes.as_bytes();
        let adj_idx = b[0] as usize % ADJECTIVES.len();
        let noun_idx = b[1] as usize % NOUNS.len();
        format!("{}-{}", ADJECTIVES[adj_idx], NOUNS[noun_idx])
    }
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::provider::{Message, MessageContent, Role};

    /// Build a `Conversation` with `n` alternating user/assistant messages.
    fn make_conv(model: &str, n: usize) -> Conversation {
        let mut conv = Conversation::new("test-id", model, AppConfig::default());
        for i in 0..n {
            if i % 2 == 0 {
                conv.messages.push(Message::user(format!("user message {i}")));
            } else {
                conv.messages.push(Message::assistant(format!("assistant message {i}")));
            }
        }
        conv
    }

    // ── test_missing_dir_created_on_save ─────────────────────────────────────
    // AC: save creates <base>/<name>/ directory automatically.

    #[test]
    fn test_missing_dir_created_on_save() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());
        let conv = make_conv("claude", 1);

        store.save("new-sess", &conv).expect("save should succeed");

        let sess_dir = tmp.path().join("new-sess");
        assert!(sess_dir.exists(), "session directory should have been created");
        assert!(sess_dir.is_dir(), "session path should be a directory");
    }

    // ── test_save_and_load_roundtrip ──────────────────────────────────────────
    // AC: save(name, &conv) then load(name) returns the same messages.

    #[test]
    fn test_save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());
        let conv = make_conv("claude-3-5-sonnet", 2);

        store.save("test-session", &conv).expect("save failed");
        let loaded = store.load("test-session").expect("load failed");

        assert_eq!(loaded.len(), 2, "should have 2 messages");
        // Check roles
        assert_eq!(loaded[0].role, Role::User);
        assert_eq!(loaded[1].role, Role::Assistant);
        // Check content
        match &loaded[0].content[0] {
            MessageContent::Text { text } => assert_eq!(text, "user message 0"),
            _ => panic!("expected text content"),
        }
        match &loaded[1].content[0] {
            MessageContent::Text { text } => assert_eq!(text, "assistant message 1"),
            _ => panic!("expected text content"),
        }
    }

    // ── test_save_creates_meta_json ───────────────────────────────────────────
    // AC: First save creates meta.json with correct name and model.

    #[test]
    fn test_save_creates_meta_json() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());
        let conv = make_conv("claude-3-5-sonnet", 1);

        store.save("swift-river", &conv).expect("save failed");

        let meta_path = tmp.path().join("swift-river").join("meta.json");
        assert!(meta_path.exists(), "meta.json should be created");

        let meta_str = std::fs::read_to_string(&meta_path).unwrap();
        let meta: MetaOnDisk = serde_json::from_str(&meta_str).expect("meta.json parse failed");
        assert_eq!(meta.name, "swift-river");
        assert_eq!(meta.model, "claude-3-5-sonnet");
        assert!(!meta.created_at.is_empty(), "created_at should be set");
    }

    // ── test_save_meta_json_idempotent ────────────────────────────────────────
    // AC: Second save does not overwrite meta.json (created_at preserved).

    #[test]
    fn test_save_meta_json_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());
        let conv = make_conv("claude", 1);

        store.save("bold-pine", &conv).expect("first save failed");

        let meta_path = tmp.path().join("bold-pine").join("meta.json");
        let meta_before: MetaOnDisk = serde_json::from_str(
            &std::fs::read_to_string(&meta_path).unwrap()
        ).unwrap();

        // Second save with more messages
        let conv2 = make_conv("claude", 3);
        store.save("bold-pine", &conv2).expect("second save failed");

        let meta_after: MetaOnDisk = serde_json::from_str(
            &std::fs::read_to_string(&meta_path).unwrap()
        ).unwrap();

        assert_eq!(
            meta_before.created_at, meta_after.created_at,
            "created_at must not change on second save"
        );
    }

    // ── test_load_nonexistent_returns_error ───────────────────────────────────
    // AC: load("no-such") returns descriptive Err.

    #[test]
    fn test_load_nonexistent_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());

        let result = store.load("no-such");
        assert!(result.is_err(), "expected Err for nonexistent session");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no-such"),
            "error should mention the session name, got: {err_msg}"
        );
    }

    // ── test_list_returns_all_sessions ────────────────────────────────────────
    // AC: After saving 2 sessions, list() returns 2 SessionMeta entries.

    #[test]
    fn test_list_returns_all_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());

        store.save("session-a", &make_conv("claude", 1)).expect("save a failed");
        store.save("session-b", &make_conv("claude", 2)).expect("save b failed");

        let sessions = store.list().expect("list failed");
        assert_eq!(sessions.len(), 2, "should list 2 sessions");

        let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"session-a"), "session-a should be listed");
        assert!(names.contains(&"session-b"), "session-b should be listed");
    }

    // ── test_list_message_count ───────────────────────────────────────────────
    // AC: message_count in SessionMeta matches number of messages saved.

    #[test]
    fn test_list_message_count() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());

        store.save("session-x", &make_conv("claude", 3)).expect("save failed");

        let sessions = store.list().expect("list failed");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].message_count, 3, "message_count should be 3");
    }

    // ── test_generate_name_format ─────────────────────────────────────────────
    // AC: generate_name() returns "word-word" matching ^[a-z]+-[a-z]+$

    #[test]
    fn test_generate_name_format() {
        for _ in 0..10 {
            let name = SessionStore::generate_name();
            let parts: Vec<&str> = name.split('-').collect();
            assert_eq!(parts.len(), 2, "name should have exactly 2 parts: {name}");
            assert!(
                parts[0].chars().all(|c| c.is_ascii_lowercase()),
                "adjective should be lowercase ascii: {name}"
            );
            assert!(
                parts[1].chars().all(|c| c.is_ascii_lowercase()),
                "noun should be lowercase ascii: {name}"
            );
        }
    }
}
