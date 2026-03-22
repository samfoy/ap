//! Session module — public interface.
//!
//! Re-exports [`Session`] and [`SessionStore`] for use across the crate.

pub mod store;

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::provider::Message;

// ─── Session ─────────────────────────────────────────────────────────────────

/// A persisted conversation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier (UUID v4 string or user-supplied).
    pub id: String,
    /// ISO 8601 timestamp of when this session was created.
    pub created_at: String,
    /// Model used for this session.
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
}

impl Session {
    /// Create a new empty session.
    pub fn new(id: String, model: String) -> Self {
        // Build a simple ISO 8601 timestamp from SystemTime (no external dep)
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Format as YYYY-MM-DDTHH:MM:SSZ (UTC, second precision)
        let created_at = format_unix_as_iso8601(secs);
        Self {
            id,
            created_at,
            model,
            messages: Vec::new(),
        }
    }

    /// Generate a new session with a random UUID id.
    pub fn generate(model: String) -> Self {
        Self::new(Uuid::new_v4().to_string(), model)
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Format a Unix timestamp (seconds) as a simple ISO 8601 UTC string.
/// e.g. `2026-03-22T14:00:00Z`
fn format_unix_as_iso8601(secs: u64) -> String {
    // Julian Day Number algorithm for calendar conversion
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    // Days since 1970-01-01 → gregorian calendar
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_new_has_empty_messages() {
        let s = Session::new("my-id".to_string(), "claude".to_string());
        assert_eq!(s.id, "my-id");
        assert_eq!(s.model, "claude");
        assert!(s.messages.is_empty());
        assert!(!s.created_at.is_empty());
    }

    #[test]
    fn test_session_generate_is_uuid() {
        let s = Session::generate("claude".to_string());
        // A UUID v4 string is 36 characters: 8-4-4-4-12
        assert_eq!(s.id.len(), 36);
    }
}
