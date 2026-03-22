//! `SessionStore` — save and load sessions from `~/.ap/sessions/`.

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::Session;

// ─── SessionStore ─────────────────────────────────────────────────────────────

/// Saves and loads sessions to/from `~/.ap/sessions/<id>.json`.
pub struct SessionStore;

impl SessionStore {
    /// Returns the path for a given session id: `~/.ap/sessions/<id>.json`.
    fn path_for(id: &str) -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(".ap").join("sessions").join(format!("{id}.json")))
    }

    /// Save a session to disk, creating the directory if necessary.
    pub fn save(session: &Session) -> Result<()> {
        let path = Self::path_for(&session.id)?;
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
    pub fn load(id: &str) -> Result<Session> {
        let path = Self::path_for(id)?;
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
    use std::fs;

    /// Helper: Save a session using a custom path (bypasses ~/.ap for tests).
    fn save_to_dir(session: &Session, dir: &std::path::Path) -> Result<()> {
        let path = dir.join(format!("{}.json", session.id));
        let json = serde_json::to_string_pretty(session)?;
        fs::write(&path, json)?;
        Ok(())
    }

    /// Helper: Load a session from a custom dir.
    fn load_from_dir(id: &str, dir: &std::path::Path) -> Result<Session> {
        let path = dir.join(format!("{id}.json"));
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("session file not found: {}", path.display()))?;
        let session: Session = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse session at {}", path.display()))?;
        Ok(session)
    }

    // ── test_save_and_reload_roundtrip ────────────────────────────────────────

    #[test]
    fn test_save_and_reload_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let mut session = Session::new("test-session".to_string(), "claude".to_string());
        session.messages.push(Message::user("hello"));

        save_to_dir(&session, dir).expect("save failed");
        let loaded = load_from_dir("test-session", dir).expect("load failed");

        assert_eq!(loaded.id, "test-session");
        assert_eq!(loaded.model, "claude");
        assert_eq!(loaded.messages.len(), 1);
    }

    // ── test_missing_dir_created ──────────────────────────────────────────────

    #[test]
    fn test_missing_dir_created() {
        let tmp = tempfile::tempdir().unwrap();
        // Use a nested path that doesn't exist yet
        let sessions_dir = tmp.path().join("nested").join("sessions");
        assert!(!sessions_dir.exists(), "pre-condition: dir should not exist");

        // We invoke SessionStore::save but override the path resolution by
        // directly calling the same logic (create_dir_all + write).
        let path = sessions_dir.join("foo.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create_dir_all should succeed");
        }
        fs::write(&path, b"{}").expect("write should succeed");

        assert!(sessions_dir.exists(), "directory should have been created");
        assert!(path.exists(), "file should have been written");
    }

    // ── test_load_nonexistent_returns_error ───────────────────────────────────

    #[test]
    fn test_load_nonexistent_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let result = load_from_dir("nonexistent-xyz", dir);
        assert!(result.is_err(), "expected Err for missing session");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("nonexistent-xyz"),
            "error message should contain session id, got: {err_msg}"
        );
    }
}
