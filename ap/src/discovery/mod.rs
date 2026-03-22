use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DiscoveryResult {
    pub tools: Vec<DiscoveredTool>,
    pub system_prompt_additions: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredTool {
    pub name: String,
    pub description: String,
    pub params: IndexMap<String, ParamSpec>,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSpec {
    pub description: String,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

// ── Private serde intermediates ───────────────────────────────────────────────

#[derive(Deserialize)]
struct ToolsFile {
    #[serde(rename = "tool", default)]
    tools: Vec<RawTool>,
}

#[derive(Deserialize)]
struct SkillFile {
    system_prompt: Option<String>,
    #[serde(rename = "tool", default)]
    tools: Vec<RawTool>,
}

#[derive(Deserialize)]
struct RawTool {
    name: String,
    description: String,
    command: String,
    #[serde(default)]
    params: IndexMap<String, ParamSpec>,
}

impl From<RawTool> for DiscoveredTool {
    fn from(r: RawTool) -> Self {
        Self {
            name: r.name,
            description: r.description,
            params: r.params,
            command: r.command,
        }
    }
}

// ── discover() ────────────────────────────────────────────────────────────────

/// Scans `root/tools.toml` and `root/.ap/skills/*.toml` and returns all
/// discovered tools, system-prompt additions, and any parse warnings.
///
/// This function is infallible: it never panics and never returns `Result`.
/// All error conditions accumulate as human-readable strings in
/// `DiscoveryResult::warnings`.
pub fn discover(root: &Path) -> DiscoveryResult {
    let mut tools: Vec<DiscoveredTool> = Vec::new();
    let mut system_prompt_additions: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // ── 1. tools.toml ─────────────────────────────────────────────────────────
    let tools_toml = root.join("tools.toml");
    if tools_toml.exists() {
        match std::fs::read_to_string(&tools_toml) {
            Err(e) => warnings.push(format!("tools.toml: {e}")),
            Ok(content) => match toml::from_str::<ToolsFile>(&content) {
                Err(e) => warnings.push(format!("tools.toml: {e}")),
                Ok(file) => {
                    for raw in file.tools {
                        add_tool(raw, "tools.toml", &mut tools, &mut seen, &mut warnings);
                    }
                }
            },
        }
    }

    // ── 2. .ap/skills/*.toml (alphabetical) ───────────────────────────────────
    let skills_dir = root.join(".ap").join("skills");
    if skills_dir.is_dir() {
        let mut entries: Vec<std::path::PathBuf> =
            std::fs::read_dir(&skills_dir).map_or_else(|_| vec![], |rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("toml"))
                    .collect()
            });
        entries.sort_by(|a, b| {
            a.file_name()
                .unwrap_or_default()
                .cmp(b.file_name().unwrap_or_default())
        });

        for path in entries {
            let display = format!(
                ".ap/skills/{}",
                path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            );
            match std::fs::read_to_string(&path) {
                Err(e) => warnings.push(format!("{display}: {e}")),
                Ok(content) => match toml::from_str::<SkillFile>(&content) {
                    Err(e) => warnings.push(format!("{display}: {e}")),
                    Ok(file) => {
                        if let Some(sp) = file.system_prompt {
                            system_prompt_additions.push(sp);
                        }
                        for raw in file.tools {
                            add_tool(raw, &display, &mut tools, &mut seen, &mut warnings);
                        }
                    }
                },
            }
        }
    }

    DiscoveryResult {
        tools,
        system_prompt_additions,
        warnings,
    }
}

/// Adds `raw` to `tools` if its name hasn't been seen; otherwise records a warning.
fn add_tool(
    raw: RawTool,
    source: &str,
    tools: &mut Vec<DiscoveredTool>,
    seen: &mut HashSet<String>,
    warnings: &mut Vec<String>,
) {
    if seen.contains(&raw.name) {
        warnings.push(format!(
            "tool '{}' in {source} conflicts with earlier definition — skipped",
            raw.name
        ));
    } else {
        seen.insert(raw.name.clone());
        tools.push(raw.into());
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(dir: &Path, rel: &str, content: &str) {
        let full = dir.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create dirs");
        }
        fs::write(&full, content).expect("write file");
    }

    // ── Step 1 serde tests (preserved) ────────────────────────────────────────

    #[test]
    fn test_tools_file_parses_correctly() {
        let toml_str = r#"
[[tool]]
name = "build"
description = "Build the project"
command = "cargo build"

[tool.params.target]
description = "Build target"
required = false
"#;
        let file: ToolsFile = toml::from_str(toml_str).expect("parse should succeed");
        assert_eq!(file.tools.len(), 1);
        let tool = &file.tools[0];
        assert_eq!(tool.name, "build");
        assert_eq!(tool.description, "Build the project");
        assert_eq!(tool.command, "cargo build");
        let param = tool.params.get("target").expect("param should exist");
        assert_eq!(param.description, "Build target");
        assert!(!param.required);
    }

    #[test]
    fn test_skill_file_parses_correctly() {
        let toml_str = r#"
system_prompt = "You are a Rust expert."

[[tool]]
name = "test"
description = "Run tests"
command = "cargo test"
"#;
        let file: SkillFile = toml::from_str(toml_str).expect("parse should succeed");
        assert_eq!(file.system_prompt, Some("You are a Rust expert.".to_string()));
        assert_eq!(file.tools.len(), 1);
        assert_eq!(file.tools[0].name, "test");
    }

    #[test]
    fn test_param_spec_required_defaults_to_true() {
        let toml_str = r#"
[[tool]]
name = "lint"
description = "Run linter"
command = "cargo clippy"

[tool.params.foo]
description = "Some param"
"#;
        let file: ToolsFile = toml::from_str(toml_str).expect("parse should succeed");
        let param = file.tools[0].params.get("foo").expect("param should exist");
        assert!(param.required, "required should default to true");
    }

    #[test]
    fn test_param_spec_required_false_explicit() {
        let toml_str = r#"
[[tool]]
name = "lint"
description = "Run linter"
command = "cargo clippy"

[tool.params.foo]
description = "Some param"
required = false
"#;
        let file: ToolsFile = toml::from_str(toml_str).expect("parse should succeed");
        let param = file.tools[0].params.get("foo").expect("param should exist");
        assert!(!param.required, "required should be false when explicitly set");
    }

    #[test]
    fn test_empty_tools_toml_does_not_error() {
        let toml_str = "";
        let file: ToolsFile = toml::from_str(toml_str).expect("empty TOML should parse fine");
        assert!(file.tools.is_empty(), "tools should be empty");
    }

    // ── Step 2: discover() tests ───────────────────────────────────────────────

    #[test]
    fn discover_empty_dir_returns_empty_result() {
        let dir = TempDir::new().unwrap();
        let result = discover(dir.path());
        assert!(result.tools.is_empty());
        assert!(result.system_prompt_additions.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn discover_valid_tools_toml_parsed_correctly() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "tools.toml",
            r#"
[[tool]]
name = "build"
description = "Build the project"
command = "cargo build"

[[tool]]
name = "test"
description = "Run tests"
command = "cargo test"
"#,
        );
        let result = discover(dir.path());
        assert_eq!(result.tools.len(), 2);
        assert_eq!(result.tools[0].name, "build");
        assert_eq!(result.tools[1].name, "test");
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn discover_malformed_tools_toml_produces_warning_no_panic() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "tools.toml", "not valid toml ][[[");
        let result = discover(dir.path());
        assert!(result.tools.is_empty());
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("tools.toml"));
    }

    #[test]
    fn discover_partial_tools_toml_skips_entire_file() {
        // One valid [[tool]] and one missing `command` field → whole file skipped
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "tools.toml",
            r#"
[[tool]]
name = "build"
description = "Build the project"
command = "cargo build"

[[tool]]
name = "broken"
description = "Missing command"
"#,
        );
        let result = discover(dir.path());
        // Whole file skipped because serde fails on the second tool
        assert!(result.tools.is_empty());
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("tools.toml"));
    }

    #[test]
    fn discover_skill_file_tools_and_system_prompt_extracted() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            ".ap/skills/ci.toml",
            r#"
system_prompt = "You are a CI expert."

[[tool]]
name = "deploy"
description = "Deploy the app"
command = "make deploy"
"#,
        );
        let result = discover(dir.path());
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "deploy");
        assert_eq!(result.system_prompt_additions.len(), 1);
        assert_eq!(result.system_prompt_additions[0], "You are a CI expert.");
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn discover_skill_files_processed_alphabetically() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            ".ap/skills/b.toml",
            r#"
[[tool]]
name = "build"
description = "Build"
command = "make build"
"#,
        );
        write_file(
            dir.path(),
            ".ap/skills/a.toml",
            r#"
[[tool]]
name = "lint"
description = "Lint"
command = "make lint"
"#,
        );
        let result = discover(dir.path());
        assert_eq!(result.tools.len(), 2);
        assert!(result.warnings.is_empty());
        // a.toml processed first → lint comes before build
        assert_eq!(result.tools[0].name, "lint");
        assert_eq!(result.tools[1].name, "build");
    }

    #[test]
    fn discover_tools_toml_wins_over_skill_file_on_duplicate() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "tools.toml",
            r#"
[[tool]]
name = "deploy"
description = "Local deploy"
command = "make deploy"
"#,
        );
        write_file(
            dir.path(),
            ".ap/skills/ci.toml",
            r#"
[[tool]]
name = "deploy"
description = "CI deploy"
command = "ci-deploy"
"#,
        );
        let result = discover(dir.path());
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].description, "Local deploy");
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("deploy"));
        assert!(result.warnings[0].contains(".ap/skills/ci.toml"));
    }

    #[test]
    fn discover_alphabetically_first_skill_wins_on_duplicate() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            ".ap/skills/a.toml",
            r#"
[[tool]]
name = "test"
description = "From a"
command = "run-a"
"#,
        );
        write_file(
            dir.path(),
            ".ap/skills/b.toml",
            r#"
[[tool]]
name = "test"
description = "From b"
command = "run-b"
"#,
        );
        let result = discover(dir.path());
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].description, "From a");
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("test"));
        assert!(result.warnings[0].contains(".ap/skills/b.toml"));
    }

    #[test]
    fn discover_system_prompt_accumulates_across_skill_files() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            ".ap/skills/a.toml",
            r#"system_prompt = "Prompt A""#,
        );
        write_file(
            dir.path(),
            ".ap/skills/b.toml",
            r#"system_prompt = "Prompt B""#,
        );
        let result = discover(dir.path());
        assert_eq!(result.system_prompt_additions.len(), 2);
        assert_eq!(result.system_prompt_additions[0], "Prompt A");
        assert_eq!(result.system_prompt_additions[1], "Prompt B");
    }

    #[test]
    fn discover_param_insertion_order_preserved() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "tools.toml",
            r#"
[[tool]]
name = "deploy"
description = "Deploy"
command = "deploy.sh"

[tool.params.c]
description = "Param c"

[tool.params.a]
description = "Param a"

[tool.params.b]
description = "Param b"
"#,
        );
        let result = discover(dir.path());
        assert_eq!(result.tools.len(), 1);
        let keys: Vec<&str> = result.tools[0].params.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["c", "a", "b"]);
    }

    #[test]
    fn discover_malformed_skill_file_produces_warning() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), ".ap/skills/bad.toml", "not valid ][[");
        let result = discover(dir.path());
        assert!(result.tools.is_empty());
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains(".ap/skills/bad.toml"));
    }

    #[test]
    fn discover_missing_skills_dir_is_silent() {
        let dir = TempDir::new().unwrap();
        // No .ap/skills directory exists
        let result = discover(dir.path());
        assert!(result.tools.is_empty());
        assert!(result.warnings.is_empty());
    }
}
