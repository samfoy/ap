use anyhow::{Context, Result};
use futures::future::BoxFuture;
use rhai::{Dynamic, Engine, Map as RhaiMap, Scope, AST};
use std::path::Path;

use crate::tools::{Tool, ToolResult};

// ─── Conversion helpers ───────────────────────────────────────────────────────

/// Convert a `serde_json::Value` to a Rhai `Dynamic`.
fn json_to_dynamic(val: serde_json::Value) -> Dynamic {
    match val {
        serde_json::Value::Null => Dynamic::UNIT,
        serde_json::Value::Bool(b) => Dynamic::from(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from(i)
            } else {
                Dynamic::from(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Dynamic::from(s),
        serde_json::Value::Array(arr) => {
            let v: rhai::Array = arr.into_iter().map(json_to_dynamic).collect();
            Dynamic::from(v)
        }
        serde_json::Value::Object(map) => {
            let m: RhaiMap = map
                .into_iter()
                .map(|(k, v)| (k.into(), json_to_dynamic(v)))
                .collect();
            Dynamic::from(m)
        }
    }
}

/// Convert a Rhai `Dynamic` to a `serde_json::Value`.
fn dynamic_to_json(val: Dynamic) -> serde_json::Value {
    if val.is::<()>() {
        serde_json::Value::Null
    } else if val.is::<bool>() {
        serde_json::Value::Bool(val.cast::<bool>())
    } else if val.is::<i64>() {
        serde_json::Value::Number(val.cast::<i64>().into())
    } else if val.is::<f64>() {
        let f = val.cast::<f64>();
        serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null)
    } else if val.is::<rhai::ImmutableString>() {
        serde_json::Value::String(val.cast::<rhai::ImmutableString>().to_string())
    } else if val.is::<rhai::Array>() {
        let arr = val.cast::<rhai::Array>();
        serde_json::Value::Array(arr.into_iter().map(dynamic_to_json).collect())
    } else if val.is::<RhaiMap>() {
        let map = val.cast::<RhaiMap>();
        let obj: serde_json::Map<String, serde_json::Value> = map
            .into_iter()
            .map(|(k, v)| (k.to_string(), dynamic_to_json(v)))
            .collect();
        serde_json::Value::Object(obj)
    } else {
        serde_json::Value::String(val.to_string())
    }
}

// ─── RhaiTool ─────────────────────────────────────────────────────────────────

/// A tool loaded from a Rhai script file.
///
/// The script must define four functions:
/// - `fn name() -> String`
/// - `fn description() -> String`
/// - `fn schema() -> Map`
/// - `fn execute(params: Map) -> Map` — returns `#{content: String, is_error: bool}`
pub struct RhaiTool {
    engine: Engine,
    ast: AST,
    name: String,
    description: String,
    schema: serde_json::Value,
}

impl RhaiTool {
    /// Load and compile a Rhai script, validating that all required functions are present.
    pub fn load(path: &Path) -> Result<Self> {
        let engine = Engine::new();
        // Engine::new() does not load file I/O or network packages — already sandboxed.

        let ast = engine
            .compile_file(path.to_path_buf())
            .with_context(|| format!("syntax error in {}", path.display()))?;

        // Call name() and description() at load time to validate they exist.
        let name: String = engine
            .call_fn(&mut Scope::new(), &ast, "name", ())
            .with_context(|| format!("missing or invalid 'name()' in {}", path.display()))?;

        let description: String = engine
            .call_fn(&mut Scope::new(), &ast, "description", ())
            .with_context(|| format!("missing or invalid 'description()' in {}", path.display()))?;

        // Validate schema() exists.
        let schema_dyn: Dynamic = engine
            .call_fn(&mut Scope::new(), &ast, "schema", ())
            .with_context(|| format!("missing or invalid 'schema()' in {}", path.display()))?;
        let schema = dynamic_to_json(schema_dyn);

        // Validate execute() exists by checking the AST's function definitions.
        // iter_functions() is the public API (iter_fn_def is gated behind `internals`).
        let has_execute = ast
            .iter_functions()
            .any(|f| f.name == "execute" && f.params.len() == 1);
        if !has_execute {
            anyhow::bail!("missing 'execute(params)' function in {}", path.display());
        }

        Ok(Self {
            engine,
            ast,
            name,
            description,
            schema,
        })
    }
}

impl Tool for RhaiTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    fn execute(&self, params: serde_json::Value) -> BoxFuture<'_, ToolResult> {
        // Convert params to Rhai Map.
        let rhai_params = match params {
            serde_json::Value::Object(map) => {
                let m: RhaiMap = map
                    .into_iter()
                    .map(|(k, v)| (k.into(), json_to_dynamic(v)))
                    .collect();
                Dynamic::from(m)
            }
            other => json_to_dynamic(other),
        };

        // Rhai is synchronous — execute inline and wrap in a ready future.
        let result = self
            .engine
            .call_fn::<Dynamic>(&mut Scope::new(), &self.ast, "execute", (rhai_params,));

        let tool_result = match result {
            Err(e) => ToolResult::err(format!("Rhai error: {e}")),
            Ok(dyn_val) => {
                if let Some(map) = dyn_val.try_cast::<RhaiMap>() {
                    let content = map
                        .get("content")
                        .map(|v: &Dynamic| v.to_string())
                        .unwrap_or_default();
                    let is_error = map
                        .get("is_error")
                        .and_then(|v: &Dynamic| v.clone().try_cast::<bool>())
                        .unwrap_or(false);
                    ToolResult { content, is_error }
                } else {
                    ToolResult::err("execute() must return a Map with 'content' and 'is_error'")
                }
            }
        };

        Box::pin(async move { tool_result })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_rhai(script: &str) -> NamedTempFile {
        let mut f = NamedTempFile::with_suffix(".rhai").unwrap();
        write!(f, "{script}").unwrap();
        f
    }

    const VALID_SCRIPT: &str = r#"
fn name() { "test_tool" }
fn description() { "A test tool" }
fn schema() { #{} }
fn execute(params) { #{content: "42", is_error: false} }
"#;

    #[test]
    fn test_load_valid_rhai_tool() {
        let f = write_rhai(VALID_SCRIPT);
        let tool = RhaiTool::load(f.path()).expect("should load");
        assert_eq!(tool.name(), "test_tool");
        assert_eq!(tool.description(), "A test tool");
    }

    #[tokio::test]
    async fn test_rhai_execute_returns_result() {
        let f = write_rhai(VALID_SCRIPT);
        let tool = RhaiTool::load(f.path()).expect("should load");
        let result = tool.execute(serde_json::json!({})).await;
        assert_eq!(result.content, "42");
        assert!(!result.is_error);
    }

    #[test]
    fn test_rhai_syntax_error_returns_err() {
        let f = write_rhai("fn name() { %%%");
        let err = RhaiTool::load(f.path());
        assert!(err.is_err(), "broken syntax should return Err");
    }

    #[test]
    fn test_rhai_missing_execute_returns_err() {
        let script = r#"
fn name() { "tool" }
fn description() { "desc" }
fn schema() { #{} }
"#;
        let f = write_rhai(script);
        let err = RhaiTool::load(f.path());
        assert!(err.is_err(), "missing execute() should return Err");
        let msg = match err {
            Err(e) => e.to_string(),
            Ok(_) => unreachable!(),
        };
        assert!(
            msg.contains("execute"),
            "error should mention execute, got: {msg}"
        );
    }

    #[test]
    fn test_rhai_missing_name_returns_err() {
        let script = r#"
fn description() { "desc" }
fn schema() { #{} }
fn execute(params) { #{content: "", is_error: false} }
"#;
        let f = write_rhai(script);
        let err = RhaiTool::load(f.path());
        assert!(err.is_err(), "missing name() should return Err");
    }
}
