pub mod runner;

pub use runner::HookRunner;

/// Outcome of running a lifecycle hook.
#[derive(Debug)]
pub enum HookOutcome {
    /// Hook not configured, or hook allowed the operation to continue.
    Proceed,
    /// Hook cancelled the operation (pre_tool_call non-zero exit).
    Cancelled(String),
    /// Hook provided replacement content (post_tool_call non-empty stdout).
    Transformed(String),
    /// Hook ran and observed but did not modify anything.
    Observed,
    /// Hook script missing, not executable, or non-fatal warning.
    HookWarning(String),
}
