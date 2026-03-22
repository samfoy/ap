# Technologies — skill-system

## Available Crates (already in Cargo.toml)

| Crate | Version | Use in skill-system |
|-------|---------|-------------------|
| `serde` | 1 (derive) | `Skill`, `SkillsConfig` serialization |
| `serde_json` | 1 | Already used throughout |
| `dirs` | 5 | `dirs::home_dir()` for `~/.ap/skills/` resolution |
| `tempfile` | 3 | Integration test tempdirs |
| `toml` | 0.8 | `overlay_from_table` extension for `[skills]` |
| `tokio` | 1 (full) | Async tests via `#[tokio::test]` |
| `anyhow` | 1 | Error handling in `main.rs` wiring |

## No New Crates Required

- Pure Rust TF-IDF: no ML crate needed
- YAML-lite frontmatter: no `serde_yaml` needed (line scanner)
- Directory listing: `std::fs::read_dir` is sufficient

## Pure Rust TF-IDF — Reference Formula

```
tokenize(text) = text.to_lowercase()
                     .split(|c: char| !c.is_alphanumeric())
                     .filter(|s| !s.is_empty())
                     .collect::<Vec<&str>>()

tf(term, doc)     = count(term in doc tokens) / doc_tokens.len()
idf(term, corpus) = f64::ln(corpus.len() as f64 / (1.0 + docs_containing_term as f64))
score(query, doc, corpus) = Σ tf(t, doc) * idf(t, corpus) for t in unique(query_terms)
```

All pure `std` — `HashMap<&str, usize>` for term frequencies.

## File I/O Pattern

```rust
// Read dir, skip missing silently
if !dir.exists() { continue; }
for entry in std::fs::read_dir(&dir)? { ... }
```

Warn on unreadable files with `eprintln!` (matches existing codebase; no `tracing` crate).

## `std::path::PathBuf` — glob-free

No glob crate — enumerate entries manually:
```rust
let entries = std::fs::read_dir(&dir).ok()?;
for entry in entries.flatten() {
    let path = entry.path();
    if path.extension().and_then(|e| e.to_str()) == Some("md") { ... }
}
```
