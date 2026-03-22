use tempfile::NamedTempFile;

use crate::config::HooksConfig;
use crate::tools::ToolResult;

use super::HookOutcome;

/// Executes lifecycle hook shell scripts and returns an outcome.
pub struct HookRunner {
    pub config: HooksConfig,
}

impl HookRunner {
    pub fn new(config: HooksConfig) -> Self {
        Self { config }
    }

    /// Run `pre_tool_call` hook.
    ///
    /// Returns `Proceed` when the hook allows the tool call to continue,
    /// `Cancelled` when it exits non-zero, and `HookWarning` when the
    /// script is missing or not executable.
    pub fn run_pre_tool_call(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> HookOutcome {
        let Some(path) = self.config.pre_tool_call.as_deref() else {
            return HookOutcome::Proceed;
        };

        if !std::path::Path::new(path).exists() {
            return HookOutcome::HookWarning(format!("hook not found: {path}"));
        }

        let params_json = params.to_string();
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(path)
            .env("AP_TOOL_NAME", tool_name)
            .env("AP_TOOL_PARAMS", &params_json)
            .output();

        match output {
            Err(e) => HookOutcome::HookWarning(format!("hook not found: {e}")),
            Ok(out) if out.status.success() => HookOutcome::Proceed,
            Ok(out) => {
                let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let msg = if msg.is_empty() {
                    "cancelled by hook".to_string()
                } else {
                    msg
                };
                HookOutcome::Cancelled(msg)
            }
        }
    }

    /// Run `post_tool_call` hook.
    ///
    /// Returns `Transformed` when the hook produces stdout, `Observed` on
    /// silent success, and `HookWarning` on non-zero exit (result unchanged).
    pub fn run_post_tool_call(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
        result: &ToolResult,
    ) -> HookOutcome {
        let Some(path) = self.config.post_tool_call.as_deref() else {
            return HookOutcome::Observed;
        };

        if !std::path::Path::new(path).exists() {
            return HookOutcome::HookWarning(format!("hook not found: {path}"));
        }

        let params_json = params.to_string();
        let result_json = serde_json::to_string(result).unwrap_or_default();
        let is_error = result.is_error.to_string();

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(path)
            .env("AP_TOOL_NAME", tool_name)
            .env("AP_TOOL_PARAMS", &params_json)
            .env("AP_TOOL_RESULT", &result_json)
            .env("AP_TOOL_IS_ERROR", &is_error)
            .output();

        match output {
            Err(e) => HookOutcome::HookWarning(format!("hook error: {e}")),
            Ok(out) if !out.status.success() => {
                let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
                HookOutcome::HookWarning(if msg.is_empty() {
                    "post_tool_call hook failed".to_string()
                } else {
                    msg
                })
            }
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if stdout.is_empty() {
                    HookOutcome::Observed
                } else {
                    HookOutcome::Transformed(stdout)
                }
            }
        }
    }

    /// Run an observer hook (pre_turn, post_turn, on_error).
    ///
    /// Observer hooks are non-cancellable. Message data is written to a
    /// temporary file and the path is exposed via `AP_MESSAGES_FILE`.
    /// Non-zero exit yields `HookWarning`; all other outcomes yield `Observed`.
    pub fn run_observer_hook(
        &self,
        hook_path: Option<&str>,
        env_vars: Vec<(String, String)>,
    ) -> HookOutcome {
        let Some(path) = hook_path else {
            return HookOutcome::Observed;
        };

        if !std::path::Path::new(path).exists() {
            return HookOutcome::HookWarning(format!("hook not found: {path}"));
        }

        // Write any message payload (from AP_MESSAGES_FILE env var value) to a temp file.
        let temp_file = NamedTempFile::new().ok();
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(path);

        for (k, v) in &env_vars {
            if k == "AP_MESSAGES_FILE" {
                // The caller passes the JSON *content* as the value; we write it
                // to a real temp file and point the env var at the path.
                if let Some(ref tf) = temp_file {
                    let _ = std::fs::write(tf.path(), v.as_bytes());
                    cmd.env(k, tf.path());
                } else {
                    cmd.env(k, v);
                }
            } else {
                cmd.env(k, v);
            }
        }

        match cmd.output() {
            Err(e) => HookOutcome::HookWarning(format!("hook error: {e}")),
            Ok(out) if !out.status.success() => {
                let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
                HookOutcome::HookWarning(if msg.is_empty() {
                    "observer hook failed".to_string()
                } else {
                    msg
                })
            }
            Ok(_) => HookOutcome::Observed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    use serde_json::json;
    use tempfile::NamedTempFile;

    use crate::config::HooksConfig;
    use crate::hooks::HookOutcome;
    use crate::tools::ToolResult;

    use super::HookRunner;

    /// Create a temporary executable shell script with the given body.
    fn make_script(body: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "{body}").unwrap();
        // Make executable
        let mut perms = f.as_file().metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.as_file().set_permissions(perms).unwrap();
        f
    }

    fn hooks_with_pre(path: &str) -> HooksConfig {
        HooksConfig {
            pre_tool_call: Some(path.to_string()),
            ..Default::default()
        }
    }

    fn hooks_with_post(path: &str) -> HooksConfig {
        HooksConfig {
            post_tool_call: Some(path.to_string()),
            ..Default::default()
        }
    }

    // ─── Test 1: pre_tool_call proceeds on exit 0 ──────────────────────────

    #[test]
    fn pre_tool_call_proceeds_on_exit_0() {
        let script = make_script("exit 0");
        let runner = HookRunner::new(hooks_with_pre(script.path().to_str().unwrap()));
        let result = runner.run_pre_tool_call("bash", &json!({"cmd": "ls"}));
        assert!(matches!(result, HookOutcome::Proceed));
    }

    // ─── Test 2: pre_tool_call cancels on non-zero exit ────────────────────

    #[test]
    fn pre_tool_call_cancels_on_nonzero_exit() {
        let script = make_script("echo 'blocked by policy' >&2; exit 1");
        let runner = HookRunner::new(hooks_with_pre(script.path().to_str().unwrap()));
        let result = runner.run_pre_tool_call("bash", &json!({}));
        match result {
            HookOutcome::Cancelled(msg) => assert!(msg.contains("blocked"), "expected 'blocked' in '{msg}'"),
            other => panic!("expected Cancelled, got {other:?}"),
        }
    }

    // ─── Test 3: post_tool_call transforms on non-empty stdout ─────────────

    #[test]
    fn post_tool_call_transforms_on_nonempty_stdout() {
        let script = make_script("echo 'transformed result'");
        let runner = HookRunner::new(hooks_with_post(script.path().to_str().unwrap()));
        let tool_result = ToolResult::ok("original");
        let result = runner.run_post_tool_call("read", &json!({}), &tool_result);
        match result {
            HookOutcome::Transformed(content) => {
                assert_eq!(content, "transformed result");
            }
            other => panic!("expected Transformed, got {other:?}"),
        }
    }

    // ─── Test 4: post_tool_call passthrough on empty stdout ────────────────

    #[test]
    fn post_tool_call_passthrough_on_empty_stdout() {
        let script = make_script("exit 0");
        let runner = HookRunner::new(hooks_with_post(script.path().to_str().unwrap()));
        let tool_result = ToolResult::ok("original");
        let result = runner.run_post_tool_call("read", &json!({}), &tool_result);
        assert!(matches!(result, HookOutcome::Observed));
    }

    // ─── Test 5: observer hook warning on non-zero exit ────────────────────

    #[test]
    fn observer_hook_warning_on_nonzero_exit() {
        let script = make_script("exit 1");
        let mut config = HooksConfig::default();
        config.pre_turn = Some(script.path().to_str().unwrap().to_string());
        let runner = HookRunner::new(config);
        let result = runner.run_observer_hook(
            runner.config.pre_turn.as_deref(),
            vec![],
        );
        assert!(matches!(result, HookOutcome::HookWarning(_)));
    }

    // ─── Test 6: hook script not found → warning ───────────────────────────

    #[test]
    fn hook_not_found_returns_warning() {
        let runner = HookRunner::new(hooks_with_pre("/nonexistent/path/hook.sh"));
        let result = runner.run_pre_tool_call("bash", &json!({}));
        match result {
            HookOutcome::HookWarning(msg) => {
                assert!(msg.contains("not found"), "expected 'not found' in '{msg}'");
            }
            other => panic!("expected HookWarning, got {other:?}"),
        }
    }
}
