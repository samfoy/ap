# Amazon Toolchain Integration — PROMPT.md

## Vision

Add first-class Amazon toolchain support to `ap`: automatic AWS profile detection, a built-in `aws` CLI tool that Claude can invoke as a tool call, Ada-vended credential refresh on 401/403, Brazil workspace awareness injected into the system prompt, and an AWS profile indicator in the TUI status bar.

The implementation follows the project's functional-first style. Everything new is either a pure function, an `impl Tool`, or a thin async wrapper around `turn()`. No global mutable state. All sub-features are individually testable and independently compilable.

---

## Architecture Overview

```
src/
  aws/
    mod.rs          — pub mod declarations + re-exports
    profile.rs      — detect_aws_profile() pure function
    ada.rs          — run_ada_refresh(), is_credential_error(), turn_with_ada_retry()
  brazil/
    mod.rs          — BrazilContext, is_brazil_workspace(), find_build_log(), detect(), brazil_system_prompt()
  tools/
    aws.rs          — AwsTool implementing Tool
  config.rs         — AwsConfig added to AppConfig
  discovery/mod.rs  — inject Brazil system prompt addition
  tui/
    mod.rs          — aws_profile: Option<String> field on TuiApp
    ui.rs           — surface AWS profile in status bar
  main.rs           — wire AwsTool, turn_with_ada_retry, aws_profile → TuiApp
```

---

## Technical Requirements

### New types and signatures

#### `config.rs` — `AwsConfig`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AwsConfig {
    /// Enable Ada-vended credential refresh on 401/403.
    pub ada_enabled: bool,
    /// AWS profile to use. "auto" means detect from environment/config.
    pub default_profile: String,
    /// Isengard account ID for Ada credential refresh. Required when ada_enabled = true.
    pub ada_account: Option<String>,
    /// IAM role name for Ada credential refresh.
    pub ada_role: Option<String>,
}

impl Default for AwsConfig {
    fn default() -> Self {
        Self {
            ada_enabled: false,
            default_profile: "auto".to_string(),
            ada_account: None,
            ada_role: None,
        }
    }
}
```

`AppConfig` gains a `pub aws: AwsConfig` field (`#[serde(default)]`). The `overlay_from_table` function handles the `[aws]` TOML table: `ada_enabled`, `default_profile`, `ada_account`, `ada_role`.

---

#### `src/aws/profile.rs`

```rust
/// Detect the active AWS profile.
///
/// Resolution order:
///   1. `AWS_PROFILE` environment variable
///   2. `AWS_DEFAULT_PROFILE` environment variable
///   3. `~/.aws/config` — returns `"default"` if the `[default]` section exists,
///      otherwise `None`.
///
/// Returns `None` if no profile can be determined.
pub fn detect_aws_profile() -> Option<String>

/// Testable variant — accepts env and config content directly.
pub fn detect_aws_profile_from(
    aws_profile_env: Option<&str>,
    aws_default_profile_env: Option<&str>,
    config_content: Option<&str>,
) -> Option<String>
```

`detect_aws_profile()` is a thin wrapper over `detect_aws_profile_from` that reads the real environment and `~/.aws/config`.

The `~/.aws/config` parser: scan for lines matching `^\[` — if `[default]` is found return `"default"`, if `[profile X]` is found return `"X"`, if only non-default profiles exist return the first one found. Keep it simple: no full INI parser needed.

---

#### `src/tools/aws.rs` — `AwsTool`

```rust
/// Built-in tool that lets Claude call the AWS CLI directly.
pub struct AwsTool {
    /// AWS profile to pass via --profile flag. None = use ambient credentials.
    profile: Option<String>,
}

impl AwsTool {
    pub fn new(profile: Option<String>) -> Self
}

impl Tool for AwsTool {
    fn name(&self) -> &str { "aws" }
    fn description(&self) -> &str { ... }
    fn schema(&self) -> serde_json::Value { ... }
    fn execute(&self, params: serde_json::Value) -> BoxFuture<'_, ToolResult>
}
```

Schema parameters:
- `subcommand` (string, required) — the AWS CLI subcommand and args, e.g. `"s3 ls"`, `"cloudwatch get-log-events --log-group-name /aws/lambda/foo"`
- `json_output` (bool, optional, default true) — appends `--output json` when true

Execution: runs `aws <subcommand> [--output json] [--profile <profile>]` via `tokio::process::Command`. When `json_output` is true and stdout is valid JSON, the result content is the pretty-printed JSON. When stdout is not valid JSON or `json_output` is false, returns stdout as-is. stderr is always appended after a `---` separator when non-empty. Exit code is appended as `exit: N`. `is_error` is always `false` — non-zero exit is captured, not a tool error (same convention as `BashTool`).

---

#### `src/aws/ada.rs`

```rust
/// Returns true if any event in `events` is a credential error.
///
/// Matches (case-insensitive) on: "ExpiredToken", "UnauthorizedException",
/// "InvalidClientTokenId", "AccessDenied", "AuthFailure", "credentials",
/// "401", "403".
pub fn is_credential_error(events: &[TurnEvent]) -> bool

/// Run `ada credentials update --provider isengard --account <account> --role <role>`.
///
/// Returns Ok(()) on exit code 0. Returns Err with captured stderr on failure.
pub async fn run_ada_refresh(account: &str, role: &str) -> anyhow::Result<()>

/// Call `turn()`, detect credential errors, optionally refresh via Ada and retry once.
///
/// `make_provider` is called at most twice — once for the initial attempt,
/// once after Ada refresh. The closure is async so it can create a new
/// `BedrockProvider` with freshly loaded credentials.
///
/// Retry only happens when:
///   - `aws_config.ada_enabled` is true
///   - `aws_config.ada_account` and `aws_config.ada_role` are both Some
///   - `is_credential_error(&events)` returns true on the first attempt
///
/// On retry failure (Ada errors, second turn errors), returns the second error.
pub async fn turn_with_ada_retry<F, Fut>(
    conv: Conversation,
    make_provider: F,
    tools: &ToolRegistry,
    middleware: &Middleware,
    aws_config: &AwsConfig,
) -> anyhow::Result<(Conversation, Vec<TurnEvent>)>
where
    F: Fn() -> Fut + Send,
    Fut: std::future::Future<Output = anyhow::Result<Arc<dyn Provider>>> + Send,
```

---

#### `src/brazil/mod.rs`

```rust
/// Snapshot of a detected Brazil workspace.
#[derive(Debug, Clone)]
pub struct BrazilContext {
    /// Absolute path to the Brazil workspace root (the dir containing `.brazil/`).
    pub workspace_root: std::path::PathBuf,
    /// Package name inferred from the workspace root directory name.
    pub package_name: String,
}

/// Returns true if `dir` looks like a Brazil workspace root.
///
/// Heuristic: the directory contains a `.brazil/` subdirectory OR a `Config`
/// file whose first 512 bytes contain the string "brazil" (case-insensitive).
pub fn is_brazil_workspace(dir: &Path) -> bool

/// Walk up from `cwd` (inclusive) to the filesystem root looking for a Brazil
/// workspace. Returns the first match.
pub fn detect(cwd: &Path) -> Option<BrazilContext>

/// Find the most recent brazil-build log file under `workspace`.
///
/// Search paths (in order):
///   1. `<workspace>/.brazil/build/` — look for `*.log` or `build.log`
///   2. `<workspace>/build/` — same
///
/// Returns the path to the most recently modified log file found, or `None`.
pub fn find_build_log(workspace: &Path) -> Option<std::path::PathBuf>

/// Generate a system prompt addition that explains Brazil conventions to Claude.
///
/// Includes:
///   - Workspace root path
///   - Package name
///   - Key commands: `brazil-build`, `brazil ws`, `brazil-recursive-cmd`
///   - Build log location hint (if `find_build_log` returns `Some`)
pub fn brazil_system_prompt(ctx: &BrazilContext) -> String
```

---

### `src/discovery/mod.rs` — Brazil injection

In `discover(root: &Path)`, after existing tool/skill loading, call `brazil::detect(root)`. If `Some(ctx)` is returned, push `brazil::brazil_system_prompt(&ctx)` into `system_prompt_additions`. No new public API — it's wired internally.

---

### TUI status bar

`TuiApp` (in `src/tui/mod.rs`) gains a `pub aws_profile: Option<String>` field. `TuiApp::new()` accepts it as a parameter.

In `render_status_bar` (in `src/tui/ui.rs`), extend the status text to include `| AWS: <profile>` when `app.aws_profile` is `Some`. It appears after the `ctx:` segment. When `None`, nothing is shown.

---

### `main.rs` wiring

1. After `AppConfig::load()`, call `detect_aws_profile()` — store as `let aws_profile: Option<String>`.
2. Register `AwsTool::new(aws_profile.clone())` in `ToolRegistry` in both `run_headless` and `run_tui`. Add it after the four defaults.
3. In `run_headless`, wrap the `turn()` call with `turn_with_ada_retry(conv_to_use, make_provider, &tools, &middleware, &config.aws)` when `config.aws.ada_enabled` is true. `make_provider` is an async closure that calls `BedrockProvider::new(config.provider.model.clone(), config.provider.region.clone())` and wraps in `Arc<dyn Provider>`.
4. In `run_tui`, pass `aws_profile` to `TuiApp::new()`.

---

## Ordered Implementation Steps

### Step 1 — `AwsConfig` + config integration

**Files:** `src/config.rs`

1. Add `AwsConfig` struct (as specified above).
2. Add `pub aws: AwsConfig` to `AppConfig` with `#[serde(default)]`.
3. Extend `overlay_from_table` with an `[aws]` block handler covering all four fields. Follow the same pattern as the existing `[context]` handler.
4. Add `src/aws/mod.rs` (initially just `pub mod profile;`) and declare `pub mod aws;` in `src/lib.rs`.

**Tests in `config.rs`:**
- `aws_config_defaults` — `AppConfig::default().aws` has `ada_enabled = false`, `default_profile = "auto"`, `ada_account = None`, `ada_role = None`
- `aws_config_toml_overlay_full` — TOML with `[aws] ada_enabled = true, default_profile = "isengard", ada_account = "123456789012", ada_role = "Admin"` parses all four fields correctly
- `aws_config_missing_keys_preserve_defaults` — TOML with only `[aws] ada_enabled = true` leaves `default_profile = "auto"`, `ada_account = None`
- `aws_config_not_serialized_in_conversation` — `Conversation` serialization round-trip still works (AwsConfig is in AppConfig which is `#[serde(default)]`)

This step must compile clean by itself: `cargo test -q 2>&1 | grep -E "^error"` produces no output.

---

### Step 2 — AWS profile detection

**Files:** `src/aws/profile.rs` (new), `src/aws/mod.rs` (add `pub mod profile; pub use profile::detect_aws_profile;`)

Implement `detect_aws_profile_from` and `detect_aws_profile` as specified above.

`~/.aws/config` parsing rules:
- Read the file; if read fails, return `None` (silent — missing config is normal)
- Scan lines for section headers matching `\[.*\]`
- If `[default]` is found anywhere → return `Some("default".to_string())`
- If a `[profile X]` line is found before `[default]` → note the name `X`
- After full scan: prefer `"default"` if found, otherwise return the first profile name found, otherwise `None`

**Tests in `src/aws/profile.rs`:**
- `profile_from_aws_profile_env` — `detect_aws_profile_from(Some("myprofile"), None, None)` → `Some("myprofile")`
- `profile_from_aws_default_profile_env` — `detect_aws_profile_from(None, Some("fallback"), None)` → `Some("fallback")`
- `profile_aws_profile_wins_over_default_profile` — both envs set → `AWS_PROFILE` wins
- `profile_from_config_default_section` — `config_content = "[default]\nregion = us-east-1\n"` → `Some("default")`
- `profile_from_config_named_profile` — `config_content = "[profile dev]\nregion = us-west-2\n"` → `Some("dev")`
- `profile_none_when_nothing_set` — all `None` → `None`
- `profile_env_wins_over_config` — env set + config present → env value returned

Compile check: `cargo test aws::profile 2>&1 | grep -E "^error"` produces no output.

---

### Step 3 — `AwsTool`

**Files:** `src/tools/aws.rs` (new), `src/tools/mod.rs` (add `pub mod aws; pub use aws::AwsTool;`)

Implement `AwsTool` as specified above.

Schema JSON:
```json
{
  "name": "aws",
  "description": "Call the AWS CLI. Returns JSON output by default. Use this to query AWS services, CloudWatch logs, S3, DynamoDB, etc.",
  "input_schema": {
    "type": "object",
    "properties": {
      "subcommand": {
        "type": "string",
        "description": "AWS CLI subcommand and arguments, e.g. \"s3 ls\", \"cloudwatch describe-log-groups\""
      },
      "json_output": {
        "type": "boolean",
        "description": "Append --output json to the command (default: true)"
      }
    },
    "required": ["subcommand"]
  }
}
```

Command construction:
```rust
let mut cmd_parts: Vec<String> = vec!["aws".to_string()];
// split subcommand on whitespace and extend
for part in subcommand.split_whitespace() {
    cmd_parts.push(part.to_string());
}
if json_output {
    cmd_parts.extend(["--output".to_string(), "json".to_string()]);
}
if let Some(profile) = &self.profile {
    cmd_parts.extend(["--profile".to_string(), profile.clone()]);
}
// run via tokio::process::Command using cmd_parts[0] + args
```

Output formatting:
```rust
let stdout_str = String::from_utf8_lossy(&output.stdout);
let stderr_str = String::from_utf8_lossy(&output.stderr);
let exit_code = output.status.code().unwrap_or(-1);

let body = if json_output {
    // Try to pretty-print JSON; fall back to raw string
    serde_json::from_str::<serde_json::Value>(stdout_str.trim())
        .map(|v| serde_json::to_string_pretty(&v).unwrap_or_else(|_| stdout_str.to_string()))
        .unwrap_or_else(|_| stdout_str.to_string())
} else {
    stdout_str.to_string()
};

let stderr_section = if stderr_str.trim().is_empty() {
    String::new()
} else {
    format!("\n---\nstderr:\n{}", stderr_str)
};

ToolResult::ok(format!("{}{}\nexit: {}", body, stderr_section, exit_code))
```

`serde_json::to_string_pretty` can't actually return `Err` for a valid `Value`, so the `unwrap_or_else` is just for Clippy compliance — handle it cleanly without triggering `unwrap_used`.

**Tests in `src/tools/aws.rs`:**
- `aws_tool_schema_has_name` — `AwsTool::new(None).schema()["name"] == "aws"`
- `aws_tool_schema_has_subcommand_required` — schema input_schema.required contains "subcommand"
- `aws_tool_missing_subcommand_returns_error` — `execute(json!({}))` returns `is_error: true`
- `aws_tool_json_output_flag_appended` — when `json_output: true`, the constructed command args contain `--output` and `json` (test by parsing the schema, not actually running aws)
- `aws_tool_profile_appended_when_set` — `AwsTool::new(Some("dev".to_string()))` — the profile is stored (unit test internal state, not subprocess)

Note: Do **not** write integration tests that actually invoke `aws` — the CI environment may not have it. Test the construction logic and schema only. Mark any tests that need `aws` in PATH with `#[ignore]`.

Compile check: `cargo test tools::aws 2>&1 | grep -E "^error"` produces no output.

---

### Step 4 — Ada credential management

**Files:** `src/aws/ada.rs` (new), `src/aws/mod.rs` (add `pub mod ada; pub use ada::{is_credential_error, run_ada_refresh, turn_with_ada_retry};`)

Implement the three functions as specified above.

`is_credential_error` implementation:
```rust
pub fn is_credential_error(events: &[TurnEvent]) -> bool {
    let markers = [
        "expiredtoken", "unauthorizedexception", "invalidclienttokenid",
        "accessdenied", "authfailure", "credentials", "401", "403",
    ];
    events.iter().any(|e| {
        if let TurnEvent::Error(msg) = e {
            let lower = msg.to_lowercase();
            markers.iter().any(|m| lower.contains(m))
        } else {
            false
        }
    })
}
```

`run_ada_refresh` implementation:
```rust
pub async fn run_ada_refresh(account: &str, role: &str) -> anyhow::Result<()> {
    let output = tokio::process::Command::new("ada")
        .args(["credentials", "update", "--provider", "isengard",
               "--account", account, "--role", role])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to spawn ada: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("ada credentials update failed: {stderr}"))
    }
}
```

`turn_with_ada_retry` must:
1. Call `make_provider().await?` to get `provider`
2. Call `turn(conv.clone(), provider.as_ref(), tools, middleware).await`
3. If `Err(e)` → check if error message is credential-related via `is_credential_error(&[TurnEvent::Error(e.to_string())])`. If yes and ada enabled → run refresh, rebuild provider, retry. Otherwise propagate.
4. If `Ok((conv2, events))` and `is_credential_error(&events)` and ada enabled with account+role → run refresh, rebuild provider, call `turn(conv.clone(), ...)` again (using the original `conv`, same input message), return that result.
5. Otherwise return `Ok((conv2, events))`.

Note: on retry, pass the **original** `conv` (before the failed turn's message was processed), not `conv2`. The failed attempt produced no valid assistant response worth keeping.

**Tests in `src/aws/ada.rs`:**
- `is_credential_error_detects_expired_token` — `[TurnEvent::Error("ExpiredTokenException: ...".to_string())]` → `true`
- `is_credential_error_detects_401` — error containing "401" → `true`
- `is_credential_error_detects_403` — error containing "403" → `true`
- `is_credential_error_detects_unauthorized` — "UnauthorizedException" → `true`
- `is_credential_error_ignores_other_errors` — `[TurnEvent::Error("network timeout")]` → `false`
- `is_credential_error_empty_events` — `[]` → `false`
- `is_credential_error_non_error_events_ignored` — `[TurnEvent::TextChunk("hello".to_string())]` → `false`
- `is_credential_error_case_insensitive` — "expiredtoken" lowercase → `true`

Compile check: `cargo test aws::ada 2>&1 | grep -E "^error"` produces no output.

---

### Step 5 — Brazil workspace detection

**Files:** `src/brazil/mod.rs` (new module), `src/lib.rs` (add `pub mod brazil;`)

Implement all four public functions as specified above.

`is_brazil_workspace` details:
```rust
pub fn is_brazil_workspace(dir: &Path) -> bool {
    // Heuristic 1: .brazil/ subdirectory
    if dir.join(".brazil").is_dir() {
        return true;
    }
    // Heuristic 2: Config file mentioning brazil
    let config_path = dir.join("Config");
    if config_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            let snippet: String = content.chars().take(512).collect();
            if snippet.to_lowercase().contains("brazil") {
                return true;
            }
        }
    }
    false
}
```

`detect` walks up the directory tree via `dir.ancestors()`.

`find_build_log` searches `.brazil/build/` and `build/` for `*.log` files, returns the most recently modified one via `std::fs::metadata().modified()`.

`brazil_system_prompt` returns a multi-line string such as:
```
## Brazil Build System

This project uses the Amazon Brazil build system.
- Workspace root: {workspace_root}
- Package: {package_name}

Key commands:
- `brazil-build` — build the current package
- `brazil ws --create --name <name>` — create a new workspace
- `brazil-recursive-cmd --cmd "brazil-build"` — build all packages in workspace
- `brazil ws use --package <pkg>` — add a package to the workspace

{build_log_hint}
```

Where `{build_log_hint}` is either empty or `"Build log found at: {path}"`.

**Tests in `src/brazil/mod.rs`** (use `tempfile::TempDir`):
- `is_brazil_workspace_detects_dot_brazil_dir` — create `.brazil/` → returns `true`
- `is_brazil_workspace_detects_config_file` — create `Config` with "brazil" in first line → `true`
- `is_brazil_workspace_false_for_empty_dir` — empty dir → `false`
- `is_brazil_workspace_false_for_unrelated_config` — `Config` without "brazil" → `false`
- `detect_finds_workspace_in_cwd` — create `.brazil/` in tempdir, call `detect(tempdir)` → `Some`
- `detect_walks_up_from_child_dir` — `.brazil/` in parent, cwd is child dir → `Some` with parent as root
- `detect_returns_none_for_non_workspace` — no markers → `None`
- `find_build_log_returns_none_when_no_build_dir` — no build dirs → `None`
- `find_build_log_returns_log_from_brazil_build` — create `.brazil/build/build.log` → `Some`
- `brazil_system_prompt_contains_workspace_root` — output contains the workspace root path string
- `brazil_system_prompt_contains_package_name` — output contains the package name

Compile check: `cargo test brazil 2>&1 | grep -E "^error"` produces no output.

---

### Step 6 — Discovery: Brazil system prompt injection

**Files:** `src/discovery/mod.rs`

At the end of `discover(root: &Path)`, before constructing `DiscoveryResult`, add:

```rust
// Brazil workspace awareness
if let Some(ctx) = crate::brazil::detect(root) {
    system_prompt_additions.push(crate::brazil::brazil_system_prompt(&ctx));
}
```

**Tests in `src/discovery/mod.rs`** (add to existing test module):
- `discover_injects_brazil_system_prompt_when_workspace_detected` — create a tempdir with `.brazil/` subdir, call `discover()` → `system_prompt_additions` contains a string with "Brazil"
- `discover_no_brazil_prompt_in_non_workspace` — empty dir → no Brazil entry in `system_prompt_additions`

Compile check: `cargo test discovery 2>&1 | grep -E "^error"` produces no output.

---

### Step 7 — TUI: AWS profile in status bar

**Files:** `src/tui/mod.rs`, `src/tui/ui.rs`

1. Add `pub aws_profile: Option<String>` to `TuiApp`.
2. Add `aws_profile: Option<String>` parameter to `TuiApp::new()`, assign to field.
3. In `render_status_bar`, extend the status string with `| AWS: {profile}` when `app.aws_profile.is_some()`. Insert it between the `ctx:` segment and the end of the string.
4. Expose a testable `format_aws_segment(profile: Option<&str>) -> String` helper in `ui.rs` (similar to existing `format_ctx_segment`).
   - `Some("myprofile")` → `"AWS: myprofile"`
   - `None` → `""` (empty string, caller skips the `|` separator)

**Tests in `src/tui/ui.rs`** (add to existing test module):
- `format_aws_segment_with_profile` — `format_aws_segment(Some("isengard"))` → `"AWS: isengard"`
- `format_aws_segment_none` — `format_aws_segment(None)` → `""`

Compile check: `cargo test tui 2>&1 | grep -E "^error"` produces no output.

---

### Step 8 — Wire everything in `main.rs`

**Files:** `src/main.rs`

After `AppConfig::load()` and before building the provider:

```rust
let aws_profile = ap::aws::detect_aws_profile();
```

In both `run_headless` and `run_tui`, after `ToolRegistry::with_defaults()`:
```rust
tools.register(Box::new(ap::tools::AwsTool::new(aws_profile.clone())));
```

In `run_headless`, replace the direct `turn()` call with `turn_with_ada_retry`. Pass a `make_provider` closure:
```rust
let model = config.provider.model.clone();
let region = config.provider.region.clone();
let make_provider = move || {
    let m = model.clone();
    let r = region.clone();
    async move {
        BedrockProvider::new(m, r)
            .await
            .map(|p| Arc::new(p) as Arc<dyn ap::provider::Provider>)
    }
};
```

Wrap the existing `turn()` call site in:
```rust
let (updated_conv, events) = if config.aws.ada_enabled {
    ap::aws::turn_with_ada_retry(conv_to_use, make_provider, &tools, &middleware, &config.aws).await
} else {
    turn(conv_to_use, initial_provider.as_ref(), &tools, &middleware).await
}
.unwrap_or_else(|e| { eprintln!("ap: error: {e}"); std::process::exit(1); });
```

Where `initial_provider` is the `Arc<dyn Provider>` built earlier (keep for the non-ada path).

In `run_tui`, pass `aws_profile` to `TuiApp::new(...)`.

Update all existing call sites for `TuiApp::new` to include the new `aws_profile` parameter.

Compile check: `cargo build 2>&1 | grep -E "^error"` produces no output.

---

## Acceptance Criteria

All of the following must be true before `LOOP_COMPLETE` is output:

### AC-1: Config
- `AppConfig::default().aws.ada_enabled == false`
- `AppConfig::default().aws.default_profile == "auto"`
- TOML `[aws] ada_enabled = true, ada_account = "123456789012", ada_role = "Admin"` round-trips correctly via `AppConfig::load_with_paths`
- Missing `[aws]` table in TOML leaves all fields at defaults (no panic, no error)

### AC-2: Profile detection
- `detect_aws_profile_from(Some("prod"), None, None)` returns `Some("prod")`
- `detect_aws_profile_from(None, None, Some("[default]\nregion=us-east-1\n"))` returns `Some("default")`
- `detect_aws_profile_from(None, None, Some("[profile dev]\nregion=us-west-2\n"))` returns `Some("dev")`
- `detect_aws_profile_from(None, None, None)` returns `None`

### AC-3: AwsTool
- `AwsTool::new(None).name() == "aws"`
- Schema contains `"subcommand"` in `required` array
- Missing `subcommand` param → `ToolResult { is_error: true }`
- `AwsTool` is registered in both headless and TUI tool registries (verifiable via `ToolRegistry::find_by_name("aws").is_some()` in a test)

### AC-4: Ada credential detection
- `is_credential_error(&[TurnEvent::Error("ExpiredTokenException".to_string())])` == `true`
- `is_credential_error(&[TurnEvent::Error("network error".to_string())])` == `false`
- `is_credential_error(&[])` == `false`
- All 8 unit tests from Step 4 pass

### AC-5: Brazil detection
- `is_brazil_workspace` correctly identifies a dir with `.brazil/` subdir
- `detect()` walks up the directory tree and finds the workspace root
- `find_build_log` returns `None` when no build dir exists and `Some(path)` when `.brazil/build/*.log` exists
- `brazil_system_prompt` output contains "brazil-build" and "brazil-recursive-cmd"

### AC-6: Discovery injection
- `discover()` on a Brazil workspace dir injects a Brazil system prompt addition
- `discover()` on a plain dir does not inject any Brazil prompt

### AC-7: TUI status bar
- `format_aws_segment(Some("isengard"))` returns `"AWS: isengard"`
- `format_aws_segment(None)` returns `""`
- `TuiApp::new(...)` accepts `aws_profile: Option<String>` parameter without compile error

### AC-8: Build and tests
- `cargo build` succeeds with zero errors
- `cargo test` passes all tests (new + existing)
- `cargo clippy -- -D warnings` produces zero warnings
- No `unwrap()`, `expect()`, or `panic!()` in non-test code (enforced by existing Cargo.toml lints)

---

## Notes for the Implementer

- All new `#[cfg(test)]` blocks must include `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` — this is the project convention.
- `run_ada_refresh` and the subprocess in `AwsTool::execute` use `tokio::process::Command` — this is already in scope via the `tokio` dependency.
- `find_build_log` uses `std::fs::metadata().modified()` which returns `Result<SystemTime>` — handle errors gracefully with `.ok()` and `?`-free iteration.
- The `turn_with_ada_retry` function should live entirely in `src/aws/ada.rs` and import `crate::turn::turn`, `crate::tools::ToolRegistry`, `crate::types::{Conversation, TurnEvent, Middleware}`, `crate::config::AwsConfig`, and `crate::provider::Provider` directly — avoid circular imports.
- `brazil_system_prompt` must not call `find_build_log` itself; accept an `Option<&Path>` for the log path or pre-call it in the caller. Keep the function pure.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and `cargo build && cargo test && cargo clippy -- -D warnings` all pass clean.
