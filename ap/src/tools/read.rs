use futures::future::BoxFuture;
use serde_json::Value;

use crate::tools::{Tool, ToolResult};

pub struct ReadTool;

impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file and return them as a string."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "read",
            "description": "Read the contents of a file and return them as a string.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute or relative path to the file to read"
                    }
                },
                "required": ["path"]
            }
        })
    }

    fn execute(&self, params: Value) -> BoxFuture<'_, ToolResult> {
        Box::pin(async move {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p.to_owned(),
                None => return ToolResult::err("missing required parameter: path"),
            };

            match tokio::fs::read_to_string(&path).await {
                Ok(contents) => ToolResult::ok(contents),
                Err(e) => ToolResult::err(format!("failed to read '{}': {}", path, e)),
            }
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_read_existing_file() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "hello").unwrap();
        let path = f.path().to_str().unwrap().to_owned();

        let result = ReadTool.execute(serde_json::json!({ "path": path })).await;
        assert!(!result.is_error);
        assert_eq!(result.content, "hello");
    }

    #[tokio::test]
    async fn test_read_missing_file_is_error() {
        let result = ReadTool
            .execute(serde_json::json!({ "path": "/tmp/ap-nonexistent-xyz-abc.txt" }))
            .await;
        assert!(result.is_error);
        assert!(
            result.content.contains("failed to read") || result.content.contains("No such file"),
            "unexpected error: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_read_missing_param_is_error() {
        let result = ReadTool.execute(serde_json::json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("missing required parameter"));
    }

    #[tokio::test]
    async fn test_read_schema_has_name() {
        let schema = ReadTool.schema();
        assert_eq!(schema["name"], "read");
    }
}
