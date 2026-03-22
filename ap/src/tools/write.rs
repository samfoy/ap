use futures::future::BoxFuture;
use serde_json::Value;

use crate::tools::{Tool, ToolResult};

pub struct WriteTool;

impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file, creating parent directories as needed."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "write",
            "description": "Write content to a file, creating parent directories as needed.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute or relative path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }
        })
    }

    fn execute(&self, params: Value) -> BoxFuture<'_, ToolResult> {
        Box::pin(async move {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p.to_owned(),
                None => return ToolResult::err("missing required parameter: path"),
            };
            let content = match params.get("content").and_then(|v| v.as_str()) {
                Some(c) => c.to_owned(),
                None => return ToolResult::err("missing required parameter: content"),
            };

            // Create parent directories if needed
            if let Some(parent) = std::path::Path::new(&path).parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return ToolResult::err(format!(
                        "failed to create parent directories for '{}': {}",
                        path, e
                    ));
                }
            }

            match tokio::fs::write(&path, &content).await {
                Ok(()) => ToolResult::ok(format!("wrote {} bytes to '{}'", content.len(), path)),
                Err(e) => ToolResult::err(format!("failed to write '{}': {}", path, e)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_write_creates_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt").to_str().unwrap().to_owned();

        let result = WriteTool
            .execute(serde_json::json!({ "path": path, "content": "hello world" }))
            .await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[tokio::test]
    async fn test_write_creates_parent_directories() {
        let dir = tempdir().unwrap();
        let path = dir
            .path()
            .join("deep/nested/dir/file.txt")
            .to_str()
            .unwrap()
            .to_owned();

        let result = WriteTool
            .execute(serde_json::json!({ "path": path, "content": "hi" }))
            .await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hi");
    }

    #[tokio::test]
    async fn test_write_overwrites_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt").to_str().unwrap().to_owned();
        std::fs::write(&path, "old content").unwrap();

        let result = WriteTool
            .execute(serde_json::json!({ "path": path, "content": "new content" }))
            .await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    }

    #[tokio::test]
    async fn test_write_schema_has_name() {
        let schema = WriteTool.schema();
        assert_eq!(schema["name"], "write");
    }
}
