use futures::future::BoxFuture;
use serde_json::Value;
use tokio::process::Command;

use crate::tools::{Tool, ToolResult};

pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a shell command via sh -c and return stdout, stderr, and exit code. No timeout in v1."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "bash",
            "description": "Run a shell command via sh -c and return stdout, stderr, and exit code. No timeout in v1.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    }
                },
                "required": ["command"]
            }
        })
    }

    fn execute(&self, params: Value) -> BoxFuture<'_, ToolResult> {
        Box::pin(async move {
            let command = match params.get("command").and_then(|v| v.as_str()) {
                Some(c) => c.to_owned(),
                None => return ToolResult::err("missing required parameter: command"),
            };

            let output = match Command::new("sh").arg("-c").arg(&command).output().await {
                Ok(o) => o,
                Err(e) => return ToolResult::err(format!("failed to spawn command: {}", e)),
            };

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);

            // Always is_error: false — non-zero exit is captured, not a tool error
            ToolResult::ok(format!("{}\n{}\nexit: {}", stdout, stderr, exit_code))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bash_captures_stdout_stderr_exit() {
        let result = BashTool
            .execute(serde_json::json!({
                "command": "echo out; echo err >&2; exit 42"
            }))
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("out"), "missing stdout: {}", result.content);
        assert!(result.content.contains("err"), "missing stderr: {}", result.content);
        assert!(
            result.content.contains("exit: 42"),
            "missing exit code: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_bash_zero_exit() {
        let result = BashTool
            .execute(serde_json::json!({ "command": "echo hello" }))
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("hello"));
        assert!(result.content.contains("exit: 0"));
    }

    #[tokio::test]
    async fn test_bash_nonzero_exit_is_not_tool_error() {
        let result = BashTool
            .execute(serde_json::json!({ "command": "exit 1" }))
            .await;
        // Non-zero exit is captured, NOT a tool-level error
        assert!(!result.is_error);
        assert!(result.content.contains("exit: 1"));
    }

    #[tokio::test]
    async fn test_bash_stdout_stderr_separated_without_trailing_newline() {
        // printf produces no trailing newline; stdout and stderr must still be
        // on separate lines so they don't concatenate into a single run-on word.
        let result = BashTool
            .execute(serde_json::json!({
                "command": "printf 'nostdoutnewline'; printf 'nostderrnewline' >&2"
            }))
            .await;
        assert!(!result.is_error);
        // The two strings must NOT be mashed together on the same line
        assert!(
            !result.content.contains("nostdoutnewlinenosterr"),
            "stdout and stderr concatenated: {}",
            result.content
        );
        assert!(result.content.contains("nostdoutnewline"), "missing stdout: {}", result.content);
        assert!(result.content.contains("nostderrnewline"), "missing stderr: {}", result.content);
    }

    #[tokio::test]
    async fn test_bash_schema_has_name() {
        let schema = BashTool.schema();
        assert_eq!(schema["name"], "bash");
    }
}
