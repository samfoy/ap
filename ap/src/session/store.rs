//! `SessionStore` — save and load sessions from a configurable directory.
//!
//! Defaults to `~/.ap/sessions/<id>.json`. Tests inject a `PathBuf` via
//! [`SessionStore::with_base`] so the real `save`/`load` code paths are
//! exercised without writing to `$HOME`.

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::Session;

// ─── SessionStore ─────────────────────────────────────────────────────────────

/// Saves and loads sessions from a directory on disk.
///
/// Use [`SessionStore::new`] for the default `~/.ap/sessions/` location, or
/// [`SessionStore::with_base`] to point at an arbitrary directory (e.g. a
/// `tempdir` in tests).
pub struct SessionStore {
    /// Root directory where session JSON files are stored.
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

    /// Returns the path for a given session id: `<base>/<id>.json`.
    fn path_for(&self, id: &str) -> PathBuf {
        self.base.join(format!("{id}.json"))
    }

    /// Save a session to disk, creating the directory if necessary.
    pub fn save(&self, session: &Session) -> Result<()> {
        let path = self.path_for(&session.id);
        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create sessions dir: {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(session)
            .context("failed to serialize session")?;
        std::fs::write(&path, json)
            .with_context(|| format!("failed to write session to {}", path.display()))?;
        Ok(())
    }

    /// Load a session from disk. Returns `Err` if the file doesn't exist or is malformed.
    pub fn load(&self, id: &str) -> Result<Session> {
        let path = self.path_for(id);
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("session file not found: {}", path.display()))?;
        let session: Session = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse session at {}", path.display()))?;
        Ok(session)
    }
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Message;

    // ── test_save_and_load_via_store ──────────────────────────────────────────
    // AC: When SessionStore::save(&session) then SessionStore::load("test-session")
    // are called, the data round-trips correctly.

    #[test]
    fn test_save_and_load_via_store() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());

        let mut session = Session::new("test-session".to_string(), "claude".to_string());
        session.messages.push(Message::user("hello"));

        store.save(&session).expect("save failed");
        let loaded = store.load("test-session").expect("load failed");

        assert_eq!(loaded.id, "test-session");
        assert_eq!(loaded.model, "claude");
        assert_eq!(loaded.messages.len(), 1);
    }

    // ── test_missing_dir_created_by_save ─────────────────────────────────────
    // AC: SessionStore::save creates parent directories automatically.

    #[test]
    fn test_missing_dir_created_by_save() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("nested").join("sessions");
        // Confirm the directory does NOT exist yet
        assert!(!nested.exists(), "pre-condition: dir should not exist");

        let store = SessionStore::with_base(nested.clone());
        let session = Session::new("foo".to_string(), "claude".to_string());
        store.save(&session).expect("save should create dirs");

        assert!(nested.exists(), "directory should have been created");
        assert!(nested.join("foo.json").exists(), "file should have been written");
    }

    // ── test_load_nonexistent_returns_error ───────────────────────────────────
    // AC: SessionStore::load returns a descriptive Err for a missing session.

    #[test]
    fn test_load_nonexistent_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_base(tmp.path().to_path_buf());

        let result = store.load("nonexistent-xyz");
        assert!(result.is_err(), "expected Err for missing session");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("nonexistent-xyz"),
            "error message should contain session id, got: {err_msg}"
        );
    }
}
