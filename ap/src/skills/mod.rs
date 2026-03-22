use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::SkillsConfig;
use crate::provider::{Message, MessageContent};
use crate::types::Conversation;

/// A skill loaded from a Markdown file on disk.
///
/// Skills are used for TF-IDF based relevance scoring and system prompt injection.
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    /// Filename without extension — used as the skill identifier.
    pub name: String,
    /// Content below the frontmatter delimiter (or entire file if no frontmatter).
    pub body: String,
    /// Tool names declared in the optional `tools:` frontmatter key.
    pub tools: Vec<String>,
}

/// Loads [`Skill`] instances from one or more directories.
///
/// Directories are processed in order; later directories override earlier ones
/// for skills with the same name (later-wins semantics, consistent with the
/// `config.toml` overlay pattern).
#[derive(Debug, Clone)]
pub struct SkillLoader {
    dirs: Vec<PathBuf>,
}

impl SkillLoader {
    /// Create a new loader that reads skills from `dirs` in order.
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self { dirs }
    }

    /// Load all skills from the configured directories.
    ///
    /// - Non-existent directories are silently skipped.
    /// - Unreadable files are skipped with a warning.
    /// - Later directories override earlier ones by skill name.
    /// - Called on every turn so file changes take effect without restart.
    pub fn load(&self) -> Vec<Skill> {
        let mut map: HashMap<String, Skill> = HashMap::new();

        for dir in &self.dirs {
            if !dir.exists() {
                continue;
            }

            let entries = match std::fs::read_dir(dir) {
                Ok(e) => e,
                Err(err) => {
                    eprintln!("ap: skills: cannot read dir {:?}: {}", dir, err);
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();

                // Only process .md files
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }

                let name = match path.file_stem().and_then(|s| s.to_str()) {
                    Some(n) => n.to_owned(),
                    None => continue,
                };

                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(err) => {
                        eprintln!("ap: skills: cannot read {:?}: {}", path, err);
                        continue;
                    }
                };

                let (tools, body) = parse_skill_file(&content);
                map.insert(name.clone(), Skill { name, body, tools });
            }
        }

        map.into_values().collect()
    }
}

/// Parse a skill file into `(tools, body)`.
///
/// If the file starts with `---\n`, the YAML-lite frontmatter is parsed until
/// the closing `---\n`. Only the `tools:` key is extracted. Everything after
/// the closing delimiter is the body.
///
/// If no frontmatter is present, the entire content is the body and `tools`
/// is empty.
fn parse_skill_file(content: &str) -> (Vec<String>, String) {
    let fm_marker = "---\n";

    if !content.starts_with(fm_marker) {
        return (vec![], content.to_owned());
    }

    // Find the closing ---
    let rest = &content[fm_marker.len()..];
    rest.find(fm_marker).map_or_else(
        || (vec![], content.to_owned()), // Malformed frontmatter — treat entire file as body
        |close_pos| {
            let frontmatter = &rest[..close_pos];
            let body = rest[close_pos + fm_marker.len()..].to_owned();
            let tools = parse_tools_from_frontmatter(frontmatter);
            (tools, body)
        },
    )
}

/// Tokenize text into lowercase alphanumeric tokens, filtering empty strings.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Select the top-`max_n` skills most relevant to the given conversation messages.
///
/// Relevance is computed using TF-IDF:
/// - TF = (term count in skill body) / (total tokens in skill body)
/// - IDF = ln(N / df + 1) where N = total skills, df = number of skills containing the term
/// - Score per skill = Σ TF × IDF for each query token present in the skill
///
/// Skills with score 0.0 (no shared tokens with the query) are excluded.
/// Returns at most `max_n` skills, sorted by descending score.
pub fn select_skills<'a>(
    skills: &'a [Skill],
    messages: &[Message],
    max_n: usize,
) -> Vec<&'a Skill> {
    if messages.is_empty() || skills.is_empty() {
        return vec![];
    }

    // Build query token set from all message content
    let query_tokens: Vec<String> = messages
        .iter()
        .flat_map(|m| {
            m.content.iter().filter_map(|c| match c {
                MessageContent::Text { text } => Some(text.as_str()),
                MessageContent::ToolUse { .. } | MessageContent::ToolResult { .. } => None,
            })
        })
        .flat_map(tokenize)
        .collect();

    if query_tokens.is_empty() {
        return vec![];
    }

    let n = skills.len() as f64;

    // Tokenize each skill body once
    let skill_tokens: Vec<Vec<String>> = skills.iter().map(|s| tokenize(&s.body)).collect();

    // Compute document frequency: for each query token, how many skills contain it?
    let mut df: HashMap<&str, usize> = HashMap::new();
    for token in &query_tokens {
        let count = skill_tokens
            .iter()
            .filter(|tokens| tokens.iter().any(|t| t == token))
            .count();
        df.entry(token.as_str()).or_insert(count);
    }

    // Score each skill
    let mut scored: Vec<(f64, &Skill)> = skills
        .iter()
        .zip(skill_tokens.iter())
        .map(|(skill, tokens)| {
            let total = tokens.len() as f64;
            if total == 0.0 {
                return (0.0, skill);
            }

            // Count term frequencies in this skill
            let mut tf_map: HashMap<&str, usize> = HashMap::new();
            for t in tokens {
                *tf_map.entry(t.as_str()).or_insert(0) += 1;
            }

            let score: f64 = query_tokens
                .iter()
                .map(|token| {
                    let tf_count = *tf_map.get(token.as_str()).unwrap_or(&0) as f64;
                    if tf_count == 0.0 {
                        return 0.0;
                    }
                    let tf = tf_count / total;
                    let doc_freq = *df.get(token.as_str()).unwrap_or(&0) as f64;
                    if doc_freq == 0.0 {
                        return 0.0;
                    }
                    let idf = (n / doc_freq + 1.0).ln();
                    tf * idf
                })
                .sum();

            (score, skill)
        })
        .filter(|(score, _)| *score > 0.0)
        .collect();

    // Sort descending by score
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    scored
        .into_iter()
        .take(max_n)
        .map(|(_, skill)| skill)
        .collect()
}

/// Format a slice of skills into a Markdown system-prompt block.
///
/// Output format:
/// ```text
/// ## Skills
///
/// ### {name}
/// {body}
/// ```
///
/// Callers must ensure the slice is non-empty; the empty-guard lives in
/// `skill_injection_middleware`.
pub fn skills_to_system_prompt(skills: &[&Skill]) -> String {
    let mut out = String::from("## Skills\n\n");
    for skill in skills {
        out.push_str(&format!("### {}\n{}", skill.name, skill.body));
        if !skill.body.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Return a `pre_turn` closure that scores skills and injects them into
/// `conv.system_prompt` before every turn.
///
/// # Behaviour
/// 1. Calls [`SkillLoader::load`] on every turn (file changes take effect immediately).
/// 2. Calls [`select_skills`] with the conversation messages and `config.max_injected`.
/// 3. If no skills score above zero, returns `None` (no modification — empty-guard).
/// 4. Otherwise builds a system-prompt block via [`skills_to_system_prompt`] and
///    returns `Some(conv.clone().with_system_prompt(block))`.
pub fn skill_injection_middleware(
    loader: SkillLoader,
    config: SkillsConfig,
) -> impl Fn(&Conversation) -> Option<Conversation> + Send + Sync + 'static {
    move |conv: &Conversation| {
        let skills = loader.load();
        let selected = select_skills(&skills, &conv.messages, config.max_injected);
        if selected.is_empty() {
            return None;
        }
        let prompt = skills_to_system_prompt(&selected);
        Some(conv.clone().with_system_prompt(prompt))
    }
}

/// Extract tool names from a `tools: [bash, read]` line in frontmatter.
fn parse_tools_from_frontmatter(frontmatter: &str) -> Vec<String> {
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("tools:") {
            let rest = rest.trim();
            // Expect [...] format
            if let Some(inner) = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                return inner
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    fn write_file(dir: &std::path::Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn skill_loader_empty_dirs() {
        let loader = SkillLoader::new(vec![]);
        let skills = loader.load();
        assert!(skills.is_empty());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn skill_loader_loads_skills() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "foo.md", "# Hello");

        let loader = SkillLoader::new(vec![dir.path().to_path_buf()]);
        let skills = loader.load();

        assert_eq!(skills.len(), 1);
        let skill = &skills[0];
        assert_eq!(skill.name, "foo");
        assert_eq!(skill.body, "# Hello");
        assert!(skill.tools.is_empty());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn skill_loader_later_dir_overrides() {
        let global = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();

        write_file(global.path(), "shared.md", "GLOBAL");
        write_file(project.path(), "shared.md", "PROJECT");

        let loader = SkillLoader::new(vec![
            global.path().to_path_buf(),
            project.path().to_path_buf(),
        ]);
        let skills = loader.load();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].body, "PROJECT");
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn skill_frontmatter_tools_parsed() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "mypkg.md", "---\ntools: [bash, read]\n---\nbody text");

        let loader = SkillLoader::new(vec![dir.path().to_path_buf()]);
        let skills = loader.load();

        assert_eq!(skills.len(), 1);
        let skill = &skills[0];
        assert_eq!(skill.tools, vec!["bash", "read"]);
        assert_eq!(skill.body.trim(), "body text");
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn skill_no_frontmatter_full_body() {
        let content = "# No Frontmatter\n\nJust content here.";
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "plain.md", content);

        let loader = SkillLoader::new(vec![dir.path().to_path_buf()]);
        let skills = loader.load();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].body, content);
        assert!(skills[0].tools.is_empty());
    }

    #[test]
    fn skill_loader_skips_nonexistent_dir() {
        let loader = SkillLoader::new(vec![PathBuf::from("/nonexistent/path/xyz")]);
        let skills = loader.load();
        assert!(skills.is_empty());
    }

    #[test]
    fn parse_skill_file_no_frontmatter() {
        let content = "hello world";
        let (tools, body) = parse_skill_file(content);
        assert!(tools.is_empty());
        assert_eq!(body, "hello world");
    }

    #[test]
    fn parse_skill_file_with_frontmatter() {
        let content = "---\ntools: [bash]\n---\nbody";
        let (tools, body) = parse_skill_file(content);
        assert_eq!(tools, vec!["bash"]);
        assert_eq!(body, "body");
    }

    #[test]
    fn parse_skill_file_malformed_frontmatter_is_full_body() {
        // Opening --- but no closing --- → treat entire file as body
        let content = "---\ntools: [bash]\nbody without closing";
        let (tools, body) = parse_skill_file(content);
        assert!(tools.is_empty());
        assert_eq!(body, content);
    }

    // ── TF-IDF tests ──────────────────────────────────────────────────────────

    fn make_skill(name: &str, body: &str) -> Skill {
        Skill { name: name.to_owned(), body: body.to_owned(), tools: vec![] }
    }

    fn make_message(text: &str) -> Message {
        Message::user(text)
    }

    #[test]
    fn select_skills_returns_top_n() {
        // Skill A: two query tokens + unique "async" term → higher TF-IDF
        // Skill B: only one query token
        // Skill C: no overlap with query
        let skills = vec![
            make_skill("a", "rust async tokio async rust"),
            make_skill("b", "rust overview"),
            make_skill("c", "python django"),
        ];
        let messages = vec![make_message("rust async")];

        let result = select_skills(&skills, &messages, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "a");
    }

    #[test]
    fn select_skills_excludes_zero_score() {
        let skills = vec![
            make_skill("match", "tokio async executor"),
            make_skill("nomatch", "python django template"),
        ];
        let messages = vec![make_message("tokio async")];

        let result = select_skills(&skills, &messages, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "match");
    }

    #[test]
    fn select_skills_empty_messages() {
        let skills = vec![make_skill("foo", "some content")];
        let result = select_skills(&skills, &[], 5);
        assert!(result.is_empty());
    }

    #[test]
    fn skills_to_system_prompt_format() {
        let skill = make_skill("foo", "bar\n");
        let result = skills_to_system_prompt(&[&skill]);
        assert_eq!(result, "## Skills\n\n### foo\nbar\n");
    }

    #[test]
    fn skills_to_system_prompt_multi_no_trailing_newline() {
        // Bodies without trailing '\n' must not merge with the next header
        let a = make_skill("a", "rust content");
        let b = make_skill("b", "python content");
        let result = skills_to_system_prompt(&[&a, &b]);
        assert_eq!(result, "## Skills\n\n### a\nrust content\n### b\npython content\n");
    }

    // ── skill_injection_middleware tests ──────────────────────────────────────

    #[test]
    #[allow(clippy::unwrap_used)]
    fn middleware_empty_skills_returns_none() {
        use crate::config::AppConfig;
        // SkillLoader with no dirs — load() returns empty
        let loader = SkillLoader::new(vec![]);
        let config = crate::config::SkillsConfig::default();
        let mw = crate::skills::skill_injection_middleware(loader, config);

        let conv = Conversation::new("id-1", "model-x", AppConfig::default())
            .with_user_message("completely unrelated message xyz123");
        // No skills to match → must return None
        assert!(mw(&conv).is_none());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn middleware_injects_system_prompt() {
        use crate::config::AppConfig;
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("docker.md"), "Run containers with docker run").unwrap();

        let loader = SkillLoader::new(vec![dir.path().to_path_buf()]);
        let config = crate::config::SkillsConfig::default();
        let mw = crate::skills::skill_injection_middleware(loader, config);

        let conv = Conversation::new("id-1", "model-x", AppConfig::default())
            .with_user_message("how do I run docker");

        let result = mw(&conv);
        assert!(result.is_some(), "expected Some(conv) with system_prompt set");
        let modified = result.unwrap();
        let sp = modified.system_prompt.as_deref().unwrap_or("");
        assert!(sp.contains("docker"), "system_prompt should contain 'docker', got: {sp}");
    }
}

