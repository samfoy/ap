# Technologies — richer-tui

## Available Crates (from Cargo.toml)

| Crate | Version | Relevance |
|-------|---------|-----------|
| `ratatui` | 0.29 | TUI framework — `Paragraph`, `Line`, `Span`, `Style`, `Color`, `Block`, `Layout` |
| `crossterm` | 0.28 (event-stream) | Terminal I/O, `KeyCode`, `KeyModifiers`, `EventStream` |
| `tokio` | 1 (full) | Async runtime, `mpsc::channel` |
| `serde_json` | 1 | `serde_json::Value` for tool params |
| `anyhow` | 1 | Error handling |
| `futures` | 0.3 | `BoxStream`, `StreamExt` |

## NO syntax highlighting crate available

No `syntect`, `tree-sitter`, or similar in Cargo.toml. The design specifies "no new crates". Code blocks get visual distinction via `Style::default().bg(Color::Rgb(30, 30, 30))` only — no token-level syntax colouring.

## Ratatui 0.29 API Notes

- `Color::Rgb(r, g, b)` — confirmed available
- `Line::from(vec![Span::...])` — standard pattern used throughout ui.rs
- `Paragraph::new(text).scroll((row, col))` — takes `(u16, u16)`; `usize::MAX as u16` = `65535` which Ratatui clamps to the available area
- `frame.set_cursor_position((x, y))` — used in render_input_box; available in 0.29
- `Block::default().borders(Borders::ALL).title(...)` — current idiom
- `Style::default().add_modifier(Modifier::BOLD)` — used in status bar

## Crossterm KeyModifiers

- `KeyModifiers::CONTROL` — used for Ctrl+C detection
- `KeyModifiers::NONE` — default
- `KeyCode::Enter` with `KeyModifiers::CONTROL` = Ctrl+Enter (may not work on all terminals — design notes this as a known compat concern)
- No `KeyModifiers::SHIFT` in current code

## Ctrl+Enter Terminal Compatibility

Many terminals send `\r` for both Enter and Ctrl+Enter. Kitty protocol / VTE terminals do distinguish them. The builder should treat Ctrl+Enter as the submit key but not break if it falls through — the existing `Enter` submit path would still work as fallback.
