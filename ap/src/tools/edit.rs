use futures::future::BoxFuture;
use serde_json::Value;

use crate::tools::{Tool, ToolResult};

pub struct EditTool;

impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Replace a unique occurrence of old_text with new_text in a file. Errors if old_text is not found or matches more than once."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "edit",
            "description": "Replace a unique occurrence of old_text with new_text in a file. Errors if old_text is not found or matches more than once.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute or relative path to the file to edit"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "The exact text to replace (must appear exactly once)"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "The replacement text"
                    }
                },
                "required": ["path", "old_text", "new_text"]
            }
        })
    }

    fn execute(&self, params: Value) -> BoxFuture<'_, ToolResult> {
        Box::pin(async move {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p.to_owned(),
                None => return ToolResult::err("missing required parameter: path"),
            };
            let old_text = match params.get("old_text").and_then(|v| v.as_str()) {
                Some(s) => s.to_owned(),
                None => return ToolResult::err("missing required parameter: old_text"),
            };
            let new_text = match params.get("new_text").and_then(|v| v.as_str()) {
                Some(s) => s.to_owned(),
                None => return ToolResult::err("missing required parameter: new_text"),
            };

            let contents = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(e) => return ToolResult::err(format!("failed to read '{}': {}", path, e)),
            };

            let count = contents.matches(old_text.as_str()).count();
            match count {
                0 => ToolResult::err("old_text not found in file"),
                1 => {
                    let updated = contents.replacen(old_text.as_str(), new_text.as_str(), 1);
                    match tokio::fs::write(&path, &updated).await {
                        Ok(()) => ToolResult::ok(format!("edited '{}' successfully", path)),
                        Err(e) => {
                            ToolResult::err(format!("failed to write '{}': {}", path, e))
                        }
                    }
                }
                n => ToolResult::err(format!(
                    "old_text matches {} occurrences (must be unique)",
                    n
                )),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(contents: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", contents).unwrap();
        f
    }

    #[tokio::test]
    async fn test_edit_replaces_unique_text() {
        let f = write_temp("hello world");
        let path = f.path().to_str().unwrap().to_owned();

        let result = EditTool
            .execute(serde_json::json!({
                "path": path,
                "old_text": "world",
                "new_text": "rust"
            }))
            .await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello rust");
    }

    #[tokio::test]
    async fn test_edit_old_text_not_found() {
        let f = write_temp("hello world");
        let path = f.path().to_str().unwrap().to_owned();

        let result = EditTool
            .execute(serde_json::json!({
                "path": path,
                "old_text": "missing",
                "new_text": "x"
            }))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("not found"), "got: {}", result.content);
    }

    #[tokio::test]
    async fn test_edit_multiple_matches_returns_error_with_count() {
        let f = write_temp("foo foo foo");
        let path = f.path().to_str().unwrap().to_owned();

        let result = EditTool
            .execute(serde_json::json!({
                "path": path,
                "old_text": "foo",
                "new_text": "bar"
            }))
            .await;
        assert!(result.is_error);
        assert!(
            result.content.contains("3 occurrences"),
            "expected '3 occurrences' in: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_edit_schema_has_name() {
        let schema = EditTool.schema();
        assert_eq!(schema["name"], "edit");
    }
}
