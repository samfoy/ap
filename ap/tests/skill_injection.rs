//! Integration test: skill injection pipeline.
//!
//! Exercises `SkillLoader → select_skills → skill_injection_middleware`
//! end-to-end without any LLM call. Uses `tempfile::TempDir` for isolation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;

use ap::config::{AppConfig, SkillsConfig};
use ap::skills::{select_skills, skill_injection_middleware, SkillLoader};
use ap::types::Conversation;

/// Full end-to-end integration test for the skill injection pipeline.
///
/// Covers:
/// 1. Later-wins directory override (project overrides global for `shared.md`)
/// 2. TF-IDF selects relevant skill (git) and excludes irrelevant one (docker)
/// 3. Middleware injects system_prompt when skills match
/// 4. Middleware returns None for empty conversation (no messages)
#[test]
fn skill_pipeline_end_to_end() {
    // ── Setup temp dirs ────────────────────────────────────────────────────
    let global_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    // Global dir: git.md and shared.md (GLOBAL version)
    fs::write(
        global_dir.path().join("git.md"),
        "Use git to commit and push changes",
    )
    .unwrap();
    fs::write(global_dir.path().join("shared.md"), "GLOBAL version").unwrap();

    // Project dir: docker.md and shared.md (PROJECT version — overrides global)
    fs::write(
        project_dir.path().join("docker.md"),
        "Use docker to build and run containers",
    )
    .unwrap();
    fs::write(project_dir.path().join("shared.md"), "PROJECT version").unwrap();

    // ── AC-1: Later-wins override ──────────────────────────────────────────
    let loader = SkillLoader::new(vec![
        global_dir.path().to_path_buf(),
        project_dir.path().to_path_buf(),
    ]);
    let skills = loader.load();

    assert_eq!(
        skills.len(),
        3,
        "expected 3 distinct skills (git, docker, shared); got {}",
        skills.len()
    );

    let shared = skills.iter().find(|s| s.name == "shared").unwrap();
    assert_eq!(
        shared.body, "PROJECT version",
        "project dir should override global for shared.md"
    );

    // ── AC-2: TF-IDF selects relevant skill ───────────────────────────────
    use ap::provider::Message;

    let messages = vec![Message::user("I need help with git commit")];
    let selected = select_skills(&skills, &messages, 5);

    let names: Vec<&str> = selected.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"git"),
        "git skill should be selected for git-related query; got: {names:?}"
    );
    assert!(
        !names.contains(&"docker"),
        "docker skill should NOT be selected for git-related query; got: {names:?}"
    );

    // ── AC-3: Middleware injects system_prompt ─────────────────────────────
    let config = SkillsConfig::default();
    let mw = skill_injection_middleware(loader.clone(), config);

    let conv = Conversation::new("test-id", "claude-3", AppConfig::default())
        .with_user_message("I need help with git commit");

    let result = mw(&conv);
    assert!(
        result.is_some(),
        "middleware should return Some(conv) when skills match"
    );
    let modified = result.unwrap();
    let system_prompt = modified.system_prompt.as_deref().unwrap_or("");
    assert!(
        system_prompt.contains("git"),
        "system_prompt should contain git skill content; got: {system_prompt:?}"
    );

    // ── AC-4: Middleware returns None for empty conversation ───────────────
    let config2 = SkillsConfig::default();
    let loader2 = SkillLoader::new(vec![
        global_dir.path().to_path_buf(),
        project_dir.path().to_path_buf(),
    ]);
    let mw2 = skill_injection_middleware(loader2, config2);

    let empty_conv = Conversation::new("test-id-2", "claude-3", AppConfig::default());
    let empty_result = mw2(&empty_conv);
    assert!(
        empty_result.is_none(),
        "middleware should return None for empty conversation (no messages)"
    );
}
