// src/middleware.rs — `Middleware` builder API and shell hook bridge.
//
// The `Middleware` struct is defined in `types.rs`. This module adds:
//   - `impl Middleware` builder methods (consuming pattern, chainable)
//   - `shell_hook_bridge()` — wraps `HooksConfig` shell scripts as middleware

use crate::config::HooksConfig;
use crate::hooks::{HookOutcome, HookRunner};
use crate::tools::ToolResult;
use crate::types::{
    Conversation, Middleware, ToolCall, ToolMiddlewareFn, ToolMiddlewareResult,
    TurnMiddlewareFn,
};

// ─── Middleware builder ────────────────────────────────────────────────────────

impl Middleware {
    /// Create an empty `Middleware` (same as `Default::default()`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a pre-tool middleware closure (consuming builder).
    pub fn pre_tool(
        mut self,
        f: impl Fn(ToolCall) -> ToolMiddlewareResult + Send + Sync + 'static,
    ) -> Self {
        self.pre_tool.push(Box::new(f) as ToolMiddlewareFn);
        self
    }

    /// Append a post-tool middleware closure (consuming builder).
    pub fn post_tool(
        mut self,
        f: impl Fn(ToolCall) -> ToolMiddlewareResult + Send + Sync + 'static,
    ) -> Self {
        self.post_tool.push(Box::new(f) as ToolMiddlewareFn);
        self
    }

    /// Append a pre-turn middleware closure (consuming builder).
    pub fn pre_turn(
        mut self,
        f: impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static,
    ) -> Self {
        self.pre_turn.push(Box::new(f) as TurnMiddlewareFn);
        self
    }

    /// Append a post-turn middleware closure (consuming builder).
    pub fn post_turn(
        mut self,
        f: impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static,
    ) -> Self {
        self.post_turn.push(Box::new(f) as TurnMiddlewareFn);
        self
    }
}

// ─── Shell hook bridge ─────────────────────────────────────────────────────────

/// Wrap a `HooksConfig` shell hook configuration as a `Middleware` chain.
///
/// Each configured hook path is adapted into a middleware closure:
/// - `pre_tool_call` → pre-tool middleware: `Cancelled` → `Block`, `Transformed` →
///   `Transform`, others → `Allow`
/// - `post_tool_call` → post-tool middleware: `Transformed` → `Transform`, others →
///   `Allow`
/// - `pre_turn` / `post_turn` → turn observers (always return `None` — no modification)
///
/// Returns `Middleware::new()` (empty, no-op) when no hooks are configured.
pub fn shell_hook_bridge(config: &HooksConfig) -> Middleware {
    let mut mw = Middleware::new();

    // pre_tool_call → pre-tool middleware
    if config.pre_tool_call.is_some() {
        let hook_config = config.clone();
        mw = mw.pre_tool(move |call: ToolCall| {
            let runner = HookRunner::new(hook_config.clone());
            let outcome = runner.run_pre_tool_call(&call.name, &call.params);
            match outcome {
                HookOutcome::Cancelled(msg) => ToolMiddlewareResult::Block(msg),
                HookOutcome::HookWarning(_) => ToolMiddlewareResult::Allow(call),
                _ => ToolMiddlewareResult::Allow(call),
            }
        });
    }

    // post_tool_call → post-tool middleware
    if config.post_tool_call.is_some() {
        let hook_config = config.clone();
        mw = mw.post_tool(move |call: ToolCall| {
            let runner = HookRunner::new(hook_config.clone());
            // post_tool middleware receives the ToolCall; we use an empty result as placeholder
            // since we only have the call at this point (result comes from execution)
            let placeholder = ToolResult::ok("");
            let outcome = runner.run_post_tool_call(&call.name, &call.params, &placeholder);
            match outcome {
                HookOutcome::Transformed(content) => {
                    ToolMiddlewareResult::Transform(ToolResult::ok(content))
                }
                HookOutcome::HookWarning(_) => ToolMiddlewareResult::Allow(call),
                _ => ToolMiddlewareResult::Allow(call),
            }
        });
    }

    // pre_turn → observer (never modifies conversation)
    if config.pre_turn.is_some() {
        let hook_config = config.clone();
        mw = mw.pre_turn(move |_conv: &Conversation| {
            let runner = HookRunner::new(hook_config.clone());
            runner.run_observer_hook(runner.config.pre_turn.as_deref(), vec![]);
            None
        });
    }

    // post_turn → observer (never modifies conversation)
    if config.post_turn.is_some() {
        let hook_config = config.clone();
        mw = mw.post_turn(move |_conv: &Conversation| {
            let runner = HookRunner::new(hook_config.clone());
            runner.run_observer_hook(runner.config.post_turn.as_deref(), vec![]);
            None
        });
    }

    mw
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use crate::config::{AppConfig, HooksConfig};
    use crate::tools::ToolResult;
    use crate::types::{Conversation, Middleware, ToolCall, ToolMiddlewareResult};

    use super::shell_hook_bridge;

    fn make_call(name: &str) -> ToolCall {
        ToolCall {
            id: "test-id".to_string(),
            name: name.to_string(),
            params: json!({"cmd": "ls"}),
        }
    }

    fn make_conv() -> Conversation {
        Conversation::new("id-1", "model-x", AppConfig::default())
    }

    /// Run the pre_tool chain against a ToolCall and return the final result.
    fn run_pre_tool(mw: &Middleware, call: ToolCall) -> ToolMiddlewareResult {
        let mut result = ToolMiddlewareResult::Allow(call);
        for f in &mw.pre_tool {
            match result {
                ToolMiddlewareResult::Allow(c) => result = f(c),
                other => return other, // Block/Transform short-circuits
            }
        }
        result
    }

    /// Run the post_tool chain against a ToolCall and return the final result.
    fn run_post_tool(mw: &Middleware, call: ToolCall) -> ToolMiddlewareResult {
        let mut result = ToolMiddlewareResult::Allow(call);
        for f in &mw.post_tool {
            match result {
                ToolMiddlewareResult::Allow(c) => result = f(c),
                other => return other,
            }
        }
        result
    }

    /// Run the pre_turn chain on a Conversation, returning the (possibly modified) conv.
    fn run_pre_turn(mw: &Middleware, conv: &Conversation) -> Conversation {
        let mut current = conv.clone();
        for f in &mw.pre_turn {
            if let Some(modified) = f(&current) {
                current = modified;
            }
        }
        current
    }

    // AC1: Pre-tool chain — all Allow → tool executes, both closures called
    #[test]
    fn pre_tool_all_allow_both_called() {
        let counter = Arc::new(Mutex::new(0u32));
        let c1 = Arc::clone(&counter);
        let c2 = Arc::clone(&counter);

        let mw = Middleware::new()
            .pre_tool(move |call| {
                *c1.lock().unwrap() += 1;
                ToolMiddlewareResult::Allow(call)
            })
            .pre_tool(move |call| {
                *c2.lock().unwrap() += 1;
                ToolMiddlewareResult::Allow(call)
            });

        let result = run_pre_tool(&mw, make_call("bash"));
        assert!(matches!(result, ToolMiddlewareResult::Allow(_)));
        assert_eq!(*counter.lock().unwrap(), 2, "both closures must run");
    }

    // AC2: Pre-tool chain — first Block stops the chain (second closure NOT called)
    #[test]
    fn pre_tool_first_block_stops_chain() {
        let second_called = Arc::new(Mutex::new(false));
        let sc = Arc::clone(&second_called);

        let mw = Middleware::new()
            .pre_tool(|_call| ToolMiddlewareResult::Block("stop".to_string()))
            .pre_tool(move |call| {
                *sc.lock().unwrap() = true;
                ToolMiddlewareResult::Allow(call)
            });

        let result = run_pre_tool(&mw, make_call("bash"));
        match result {
            ToolMiddlewareResult::Block(msg) => assert_eq!(msg, "stop"),
            other => panic!("expected Block, got {other:?}"),
        }
        assert!(!*second_called.lock().unwrap(), "second closure must NOT run");
    }

    // AC3: Post-tool Transform overrides result
    #[test]
    fn post_tool_transform_overrides() {
        let mw = Middleware::new()
            .post_tool(|_call| ToolMiddlewareResult::Transform(ToolResult::ok("override")));

        let result = run_post_tool(&mw, make_call("read"));
        match result {
            ToolMiddlewareResult::Transform(r) => {
                assert_eq!(r.content, "override");
                assert!(!r.is_error);
            }
            other => panic!("expected Transform, got {other:?}"),
        }
    }

    // AC4: Pre-turn chain modifies Conversation
    #[test]
    fn pre_turn_modifies_conversation() {
        let mw = Middleware::new().pre_turn(|conv: &Conversation| {
            // Return a conversation with a modified model name as a marker
            let mut modified = conv.clone();
            modified.model = "modified-model".to_string();
            Some(modified)
        });

        let conv = make_conv();
        let result = run_pre_turn(&mw, &conv);
        assert_eq!(result.model, "modified-model");
    }

    // AC5: shell_hook_bridge — no-op for empty HooksConfig
    #[test]
    fn shell_hook_bridge_empty_config_is_noop() {
        let config = HooksConfig {
            pre_tool_call: None,
            post_tool_call: None,
            pre_turn: None,
            post_turn: None,
            on_error: None,
        };
        let mw = shell_hook_bridge(&config);
        assert!(mw.pre_tool.is_empty(), "pre_tool chain must be empty");
        assert!(mw.post_tool.is_empty(), "post_tool chain must be empty");
        assert!(mw.pre_turn.is_empty(), "pre_turn chain must be empty");
        assert!(mw.post_turn.is_empty(), "post_turn chain must be empty");

        // Running on a call still returns Allow (no-op)
        let result = run_pre_tool(&mw, make_call("bash"));
        assert!(matches!(result, ToolMiddlewareResult::Allow(_)));
    }

    // AC6: shell_hook_bridge — Block on HookOutcome::Cancelled
    #[test]
    fn shell_hook_bridge_block_on_cancelled() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use tempfile::NamedTempFile;

        // Script that exits non-zero (triggers Cancelled in HookRunner)
        let mut script = NamedTempFile::new().unwrap();
        writeln!(script, "#!/bin/sh").unwrap();
        writeln!(script, "echo 'blocked by policy' >&2; exit 1").unwrap();
        let mut perms = script.as_file().metadata().unwrap().permissions();
        perms.set_mode(0o755);
        script.as_file().set_permissions(perms).unwrap();

        let config = HooksConfig {
            pre_tool_call: Some(script.path().to_str().unwrap().to_string()),
            post_tool_call: None,
            pre_turn: None,
            post_turn: None,
            on_error: None,
        };

        let mw = shell_hook_bridge(&config);
        assert_eq!(mw.pre_tool.len(), 1, "one pre_tool middleware expected");

        let result = run_pre_tool(&mw, make_call("bash"));
        match result {
            ToolMiddlewareResult::Block(msg) => {
                assert!(msg.contains("blocked"), "expected 'blocked' in '{msg}'");
            }
            other => panic!("expected Block, got {other:?}"),
        }
    }
}
