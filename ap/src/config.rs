use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ─── Sub-config structs ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub backend: String,
    pub model: String,
    pub region: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            backend: "bedrock".to_string(),
            model: "us.anthropic.claude-sonnet-4-6".to_string(),
            region: "us-west-2".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HooksConfig {
    pub pre_tool_call: Option<String>,
    pub post_tool_call: Option<String>,
    pub pre_turn: Option<String>,
    pub post_turn: Option<String>,
    pub on_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub enabled: Vec<String>,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            enabled: vec![
                "read".to_string(),
                "write".to_string(),
                "edit".to_string(),
                "bash".to_string(),
            ],
        }
    }
}

/// Configuration for the skill injection middleware.
#[derive(Debug, Clone)]
pub struct SkillsConfig {
    /// Whether skill injection is enabled.
    pub enabled: bool,
    /// Maximum number of skills to inject per turn.
    pub max_injected: usize,
    /// Explicit skill directories. `None` means use the default dirs
    /// (`~/.ap/skills/` and `./ap-skills/`).
    pub dirs: Option<Vec<PathBuf>>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_injected: 5,
            dirs: None,
        }
    }
}

// ─── Top-level AppConfig ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub provider: ProviderConfig,
    pub hooks: HooksConfig,
    pub tools: ToolsConfig,
    #[serde(skip)]
    pub skills: SkillsConfig,
}

// ─── Fine-grained table overlay ───────────────────────────────────────────

/// Apply only the fields present in `table` onto `base`.
///
/// TOML `#[serde(default)]` fills every absent field with its default,
/// making it impossible to distinguish "user wrote this" from "serde
/// filled in the default". We therefore overlay from the raw `toml::Table`
/// so that only explicitly written fields override the base.
fn overlay_from_table(mut base: AppConfig, table: toml::Table) -> AppConfig {
    if let Some(toml::Value::Table(pt)) = table.get("provider") {
        if let Ok(p) = toml::Value::Table(pt.clone()).try_into::<ProviderConfig>() {
            if pt.contains_key("backend") {
                base.provider.backend = p.backend;
            }
            if pt.contains_key("model") {
                base.provider.model = p.model;
            }
            if pt.contains_key("region") {
                base.provider.region = p.region;
            }
        }
    }
    if let Some(toml::Value::Table(ht)) = table.get("hooks") {
        if let Ok(h) = toml::Value::Table(ht.clone()).try_into::<HooksConfig>() {
            if ht.contains_key("pre_tool_call") {
                base.hooks.pre_tool_call = h.pre_tool_call;
            }
            if ht.contains_key("post_tool_call") {
                base.hooks.post_tool_call = h.post_tool_call;
            }
            if ht.contains_key("pre_turn") {
                base.hooks.pre_turn = h.pre_turn;
            }
            if ht.contains_key("post_turn") {
                base.hooks.post_turn = h.post_turn;
            }
            if ht.contains_key("on_error") {
                base.hooks.on_error = h.on_error;
            }
        }
    }
    if let Some(toml::Value::Table(tt)) = table.get("tools") {
        if let Ok(t) = toml::Value::Table(tt.clone()).try_into::<ToolsConfig>() {
            if tt.contains_key("enabled") {
                base.tools.enabled = t.enabled;
            }
        }
    }
    if let Some(toml::Value::Table(st)) = table.get("skills") {
        if st.contains_key("enabled") {
            if let Some(toml::Value::Boolean(v)) = st.get("enabled") {
                base.skills.enabled = *v;
            }
        }
        if st.contains_key("max_injected") {
            if let Some(toml::Value::Integer(v)) = st.get("max_injected") {
                base.skills.max_injected = usize::try_from(*v).unwrap_or(0);
            }
        }
        if st.contains_key("dirs") {
            if let Some(toml::Value::Array(arr)) = st.get("dirs") {
                let dirs: Vec<PathBuf> = arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(PathBuf::from)
                    .collect();
                base.skills.dirs = Some(dirs);
            }
        }
    }
    base
}

// ─── File loader ──────────────────────────────────────────────────────────

/// Load a TOML config file as a raw table. Returns `None` if the file
/// does not exist. Returns `Err` with path context on parse failure.
pub fn load_file(path: &Path) -> Result<Option<toml::Table>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;
    let table: toml::Table = toml::from_str(&raw)
        .with_context(|| format!("failed to parse config file at {}", path.display()))?;
    Ok(Some(table))
}

// ─── AppConfig load ───────────────────────────────────────────────────────

impl AppConfig {
    /// Load configuration by merging global (`~/.ap/config.toml`) and
    /// project (`./ap.toml`) configs. Project overrides global.
    /// Both files are optional — returns defaults if neither exists.
    pub fn load() -> Result<AppConfig> {
        Self::load_with_paths(
            dirs::home_dir()
                .map(|h| h.join(".ap").join("config.toml"))
                .as_deref(),
            Some(Path::new("ap.toml")),
        )
    }

    /// Testable variant that accepts explicit paths.
    pub fn load_with_paths(
        global_path: Option<&Path>,
        project_path: Option<&Path>,
    ) -> Result<AppConfig> {
        let mut config = AppConfig::default();

        // Apply global config (if present)
        if let Some(gp) = global_path {
            if let Some(table) = load_file(gp)? {
                config = overlay_from_table(config, table);
            }
        }

        // Apply project config on top (project wins)
        if let Some(pp) = project_path {
            if let Some(table) = load_file(pp)? {
                config = overlay_from_table(config, table);
            }
        }

        Ok(config)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_toml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", content).unwrap();
        f
    }

    #[test]
    fn test_defaults_when_no_file() {
        // No config files → all defaults returned
        let cfg = AppConfig::load_with_paths(None, None).unwrap();
        assert_eq!(cfg.provider.model, "us.anthropic.claude-sonnet-4-6");
        assert_eq!(cfg.provider.backend, "bedrock");
        assert_eq!(cfg.provider.region, "us-west-2");
        assert!(cfg.hooks.pre_tool_call.is_none());
        assert_eq!(cfg.tools.enabled, vec!["read", "write", "edit", "bash"]);
    }

    #[test]
    fn test_load_project_config() {
        let project = write_toml(
            r#"
[provider]
model = "custom-model"
"#,
        );
        let cfg = AppConfig::load_with_paths(None, Some(project.path())).unwrap();
        assert_eq!(cfg.provider.model, "custom-model");
        // Non-overridden fields keep their defaults
        assert_eq!(cfg.provider.backend, "bedrock");
        assert_eq!(cfg.provider.region, "us-west-2");
    }

    #[test]
    fn test_global_config_merged() {
        let global = write_toml(
            r#"
[provider]
model = "global-model"
"#,
        );
        let cfg = AppConfig::load_with_paths(Some(global.path()), None).unwrap();
        assert_eq!(cfg.provider.model, "global-model");
        assert_eq!(cfg.provider.backend, "bedrock");
    }

    #[test]
    fn test_project_overrides_global() {
        let global = write_toml(
            r#"
[provider]
model = "global-model"
region = "eu-west-1"
"#,
        );
        let project = write_toml(
            r#"
[provider]
model = "project-model"
"#,
        );
        let cfg =
            AppConfig::load_with_paths(Some(global.path()), Some(project.path())).unwrap();
        // Project wins for model
        assert_eq!(cfg.provider.model, "project-model");
        // Global wins for region (project didn't set it)
        assert_eq!(cfg.provider.region, "eu-west-1");
    }

    #[test]
    fn test_invalid_toml_returns_error() {
        let bad = write_toml("[invalid toml %%%");
        let result = AppConfig::load_with_paths(None, Some(bad.path()));
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        // Error message must contain the file path
        assert!(
            err_msg.contains(bad.path().to_str().unwrap()),
            "error message should contain file path, got: {}",
            err_msg
        );
    }

    // ── SkillsConfig tests ──────────────────────────────────────────────

    #[test]
    fn skills_config_default() {
        let cfg = SkillsConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.max_injected, 5);
        assert!(cfg.dirs.is_none());
    }

    #[test]
    fn skills_config_toml_overlay() {
        let project = write_toml(
            r#"
[skills]
max_injected = 3
enabled = false
"#,
        );
        let cfg = AppConfig::load_with_paths(None, Some(project.path())).unwrap();
        assert_eq!(cfg.skills.max_injected, 3);
        assert!(!cfg.skills.enabled);
        // dirs not set — should remain None
        assert!(cfg.skills.dirs.is_none());
    }

    #[test]
    fn skills_config_missing_keys_preserve_defaults() {
        let project = write_toml("[skills]\n");
        let cfg = AppConfig::load_with_paths(None, Some(project.path())).unwrap();
        assert!(cfg.skills.enabled);
        assert_eq!(cfg.skills.max_injected, 5);
        assert!(cfg.skills.dirs.is_none());
    }

    #[test]
    fn skills_config_dirs_overlay() {
        let project = write_toml(
            r#"
[skills]
dirs = ["/tmp/skills", "/home/user/skills"]
"#,
        );
        let cfg = AppConfig::load_with_paths(None, Some(project.path())).unwrap();
        let dirs = cfg.skills.dirs.unwrap();
        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0], PathBuf::from("/tmp/skills"));
        assert_eq!(dirs[1], PathBuf::from("/home/user/skills"));
    }

    #[test]
    fn skills_config_negative_max_injected_becomes_zero() {
        let project = write_toml(
            r#"
[skills]
max_injected = -1
"#,
        );
        let cfg = AppConfig::load_with_paths(None, Some(project.path())).unwrap();
        assert_eq!(cfg.skills.max_injected, 0);
    }
}
