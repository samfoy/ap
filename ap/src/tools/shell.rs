use std::path::PathBuf;

use futures::future::BoxFuture;
use serde_json::{Value, json};

use crate::discovery::DiscoveredTool;
use crate::tools::{Tool, ToolResult};

pub struct ShellTool {
    tool: DiscoveredTool,
    root: PathBuf,
}

impl ShellTool {
    pub fn new(tool: DiscoveredTool, root: PathBuf) -> Self {
        Self { tool, root }
    }
}

impl Tool for ShellTool {
    fn name(&self) -> &str {
        &self.tool.name
    }

    fn description(&self) -> &str {
        &self.tool.description
    }

    fn schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required: Vec<Value> = Vec::new();

        for (key, spec) in &self.tool.params {
            properties.insert(
                key.clone(),
                json!({
                    "type": "string",
                    "description": spec.description
                }),
            );
            if spec.required {
                required.push(json!(key));
            }
        }

        json!({
            "name": self.tool.name,
            "description": self.tool.description,
            "input_schema": {
                "type": "object",
                "properties": properties,
                "required": required
            }
        })
    }

    fn execute(&self, params: Value) -> BoxFuture<'_, ToolResult> {
        let command = self.tool.command.clone();
        let root = self.root.clone();

        // Validate required params and build env vars
        let mut env_vars: Vec<(String, String)> = Vec::new();
        for (key, spec) in &self.tool.params {
            let env_key = format!("AP_PARAM_{}", key.to_uppercase());
            match params.get(key).and_then(|v| v.as_str()) {
                Some(val) => env_vars.push((env_key, val.to_owned())),
                None if spec.required => {
                    let msg = format!("missing required parameter: {key}");
                    return Box::pin(async move { ToolResult::err(msg) });
                }
                None => {} // optional, skip
            }
        }

        Box::pin(async move {
            let output = match std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .envs(env_vars)
                .current_dir(&root)
                .output()
            {
                Ok(o) => o,
                Err(e) => return ToolResult::err(format!("failed to spawn command: {e}")),
            };

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);

            ToolResult::ok(format!("{stdout}\n{stderr}\nexit: {exit_code}"))
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use indexmap::IndexMap;

    use super::*;
    use crate::discovery::ParamSpec;

    fn make_tool(name: &str, command: &str, params: IndexMap<String, ParamSpec>) -> ShellTool {
        let discovered = DiscoveredTool {
            name: name.to_owned(),
            description: "test tool".to_owned(),
            params,
            command: command.to_owned(),
        };
        ShellTool::new(discovered, std::env::temp_dir())
    }

    fn req(description: &str) -> ParamSpec {
        ParamSpec { description: description.to_owned(), required: true }
    }

    fn opt(description: &str) -> ParamSpec {
        ParamSpec { description: description.to_owned(), required: false }
    }

    // AC1: required params appear in schema's required array, optional do not
    #[test]
    fn schema_required_params_in_required_array() {
        let mut params = IndexMap::new();
        params.insert("user".to_owned(), req("user name"));
        params.insert("format".to_owned(), req("output format"));
        params.insert("verbose".to_owned(), opt("verbose flag"));
        let tool = make_tool("greet", "echo hi", params);
        let schema = tool.schema();
        let required = schema["input_schema"]["required"].as_array().unwrap();
        let required_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_names.contains(&"user"), "user missing from required");
        assert!(required_names.contains(&"format"), "format missing from required");
        assert!(!required_names.contains(&"verbose"), "verbose should not be required");
    }

    // AC2: optional params in properties but not in required
    #[test]
    fn schema_optional_params_in_properties_not_required() {
        let mut params = IndexMap::new();
        params.insert("verbose".to_owned(), opt("verbose flag"));
        let tool = make_tool("opt_tool", "echo hi", params);
        let schema = tool.schema();
        assert!(
            schema["input_schema"]["properties"]["verbose"].is_object(),
            "verbose should be in properties"
        );
        let required = schema["input_schema"]["required"].as_array().unwrap();
        let required_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(!required_names.contains(&"verbose"), "verbose should not be required");
    }

    // AC3: execute with all required params succeeds
    #[tokio::test]
    async fn execute_with_required_params_succeeds() {
        let mut params = IndexMap::new();
        params.insert("foo".to_owned(), req("foo param"));
        let tool = make_tool("echo_foo", "echo $AP_PARAM_FOO", params);
        let result = tool.execute(serde_json::json!({"foo": "bar"})).await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert!(result.content.contains("bar"), "expected 'bar' in: {}", result.content);
    }

    // AC4: env var key is uppercased
    #[tokio::test]
    async fn env_var_key_is_uppercased() {
        let mut params = IndexMap::new();
        params.insert("my_key".to_owned(), req("my key param"));
        let tool = make_tool("upper_tool", "echo $AP_PARAM_MY_KEY", params);
        let result = tool.execute(serde_json::json!({"my_key": "hello"})).await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert!(result.content.contains("hello"), "expected 'hello' in: {}", result.content);
    }

    // AC5: missing required param returns error
    #[tokio::test]
    async fn missing_required_param_returns_error() {
        let mut params = IndexMap::new();
        params.insert("foo".to_owned(), req("foo param"));
        let tool = make_tool("missing_tool", "echo $AP_PARAM_FOO", params);
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_error, "expected error for missing param");
        assert!(
            result.content.contains("missing required parameter: foo"),
            "unexpected error message: {}",
            result.content
        );
    }

    // AC6: optional param absent runs successfully
    #[tokio::test]
    async fn optional_param_absent_runs_successfully() {
        let mut params = IndexMap::new();
        params.insert("opt".to_owned(), opt("optional param"));
        let tool = make_tool("opt_tool", "echo ${AP_PARAM_OPT:-default}", params);
        let result = tool.execute(serde_json::json!({})).await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert!(result.content.contains("default"), "expected 'default' in: {}", result.content);
    }

    // AC7: non-zero exit code is not a tool error
    #[tokio::test]
    async fn nonzero_exit_is_not_tool_error() {
        let tool = make_tool("exit_tool", "exit 1", IndexMap::new());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(!result.is_error, "non-zero exit should not be tool error");
        assert!(result.content.contains("exit: 1"), "expected 'exit: 1' in: {}", result.content);
    }

    // AC8: spawn failure returns error (use a root dir that doesn't exist to force failure)
    #[tokio::test]
    async fn spawn_failure_returns_error() {
        let discovered = DiscoveredTool {
            name: "bad_tool".to_owned(),
            description: "test".to_owned(),
            params: IndexMap::new(),
            command: "echo hi".to_owned(),
        };
        // Use a non-existent directory to force spawn failure (current_dir will fail)
        let tool = ShellTool::new(discovered, PathBuf::from("/nonexistent/directory/that/does/not/exist"));
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_error, "expected spawn error");
        assert!(
            result.content.contains("failed to spawn"),
            "expected 'failed to spawn' in: {}",
            result.content
        );
    }

    // AC9: command runs in root dir
    #[tokio::test]
    async fn command_runs_in_root_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let discovered = DiscoveredTool {
            name: "pwd_tool".to_owned(),
            description: "test".to_owned(),
            params: IndexMap::new(),
            command: "pwd".to_owned(),
        };
        let tool = ShellTool::new(discovered, root.clone());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        // Resolve symlinks for comparison (macOS /tmp is symlinked)
        let resolved = root.canonicalize().unwrap();
        assert!(
            result.content.contains(resolved.to_str().unwrap()),
            "expected root dir '{}' in: {}",
            resolved.display(),
            result.content
        );
    }
}
