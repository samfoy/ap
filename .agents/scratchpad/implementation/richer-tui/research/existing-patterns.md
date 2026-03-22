# Existing Patterns — richer-tui

## File Attribution

| File | Key Lines |
|------|-----------|
| `ap/src/types.rs` | 50-68 TurnEvent, 130-150 tests |
| `ap/src/turn.rs` | 130-145 StreamEvent::TurnEnd arm, 160-170 TurnEnd push |
| `ap/src/tui/mod.rs` | 50-90 TuiApp struct, 130-180 handle_ui_event, 200-250 headless() |
| `ap/src/tui/events.rs` | 24-90 handle_key_event, 95-150 tests |
| `ap/src/tui/ui.rs` | 15-30 render(), 50-60 render_status_bar, 65-90 render_conversation |

---

## Pattern 1 — TurnEvent is a plain enum with no serde (just Debug + Clone)

**File:** `ap/src/types.rs:50-68`

```rust
#[derive(Debug, Clone)]
pub enum TurnEvent {
    TextChunk(String),
    ToolStart { name: String, params: serde_json::Value },
    ToolComplete { name: String, result: String },
    TurnEnd,
    Error(String),
}
```

→ `Usage { input_tokens: u32, output_tokens: u32 }` follows the same named-field style as `ToolStart` / `ToolComplete`. No serde needed.

---

## Pattern 2 — StreamEvent::TurnEnd already carries token counts (discarded)

**File:** `ap/src/turn.rs:140-145`

```rust
StreamEvent::TurnEnd { .. } => {
    conv = apply_post_turn(conv, middleware);
    break;
}
```

The `..` discards `input_tokens` and `output_tokens`. Step 1 just changes `..` to `{ input_tokens, output_tokens, .. }` and pushes a `TurnEvent::Usage` before the break.

---

## Pattern 3 — TurnEnd is emitted in two places (no-tool and post-loop)

**File:** `ap/src/turn.rs:160-168`

```rust
if pending_tools.is_empty() {
    all_events.push(TurnEvent::TurnEnd);
    return Ok((conv, all_events));
}
```

Also, `StreamEvent::TurnEnd` causes `apply_post_turn` then `break`, then the outer loop checks `pending_tools.is_empty()`. So token usage comes from `StreamEvent::TurnEnd`, and `TurnEvent::TurnEnd` is pushed separately after tool execution is complete.

→ `TurnEvent::Usage` should be pushed inside the `StreamEvent::TurnEnd` arm, before `break`. `TurnEvent::TurnEnd` stays where it is.

---

## Pattern 4 — TuiApp::headless() mirrors TuiApp::new() field-by-field

**File:** `ap/src/tui/mod.rs:130-180`

Every field in `TuiApp::new()` has a matching initializer in `headless()`. Each step that adds fields to `TuiApp` MUST update `headless()` with the same defaults, or tests won't compile.

---

## Pattern 5 — handle_ui_event matches on TurnEvent variants

**File:** `ap/src/tui/mod.rs:200-230`

```rust
pub fn handle_ui_event(&mut self, event: TurnEvent) {
    match event {
        TurnEvent::TextChunk(text) => { ... }
        TurnEvent::ToolStart { name, params } => { ... }
        TurnEvent::ToolComplete { name, result } => { ... }
        TurnEvent::TurnEnd => { ... }
        TurnEvent::Error(e) => { ... }
    }
}
```

Step 1 adds `TurnEvent::Usage { input_tokens, output_tokens } => { ... }` arm. The match is exhaustive — compiler will catch missing arms.

---

## Pattern 6 — events.rs returns Action enum; app state mutation is in-place

**File:** `ap/src/tui/events.rs:24-90`

`handle_key_event` takes `&mut TuiApp`, mutates it directly for mode/scroll changes, and returns `Action`. Pattern: mutation happens in the handler, `Action` signals async operations (submit, quit) back to event loop.

---

## Pattern 7 — render functions are private; tests use headless app + handle_ui_event

**File:** `ap/src/tui/mod.rs:tests`, `ap/src/tui/events.rs:tests`

Tests for TUI state use `TuiApp::headless()` + `handle_ui_event()` / `handle_key_event()`. Render functions (`render_status_bar`, `render_conversation`, etc.) are private and NOT tested directly. The design's `code_block_lines_have_dark_bg` test requires a **public free function** — `parse_chat_blocks` is always public, and the builder may need a public `chat_blocks_to_lines` as well.

---

## Pattern 8 — Clippy strict mode: no unwrap/expect outside #[cfg(test)]

**File:** `ap/Cargo.toml`

```toml
[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
```

All new production code must use `?`, `unwrap_or`, `unwrap_or_default`, `if let`, etc. `unwrap()` / `expect()` / `panic!()` only inside `#[cfg(test)]` blocks (which have `#[allow(...)]`).

---

## Pattern 9 — Ratatui widget idiom

**File:** `ap/src/tui/ui.rs:65-90`

Paragraph → block → borders/title → render to frame. Layout constraints via `Constraint::Length` / `Percentage` / `Min`. Style via `Style::default().bg(...).fg(...).add_modifier(...)`.

The `scroll` method takes `(u16, u16)` — current usage: `(app.scroll_offset as u16, 0)`. For Step 5, `scroll_offset = usize::MAX` means clamped to `u16::MAX` when cast.

---

## Pattern 10 — is_error detection for ToolComplete is string-based (fragile)

**File:** `ap/src/tui/mod.rs:186-196`

```rust
let icon = if result.starts_with("error") || result.contains("blocked") {
    "✗"
} else {
    "✓"
};
```

Step 3 replaces this with `ToolEntry.is_error: bool` pulled directly from `TurnEvent::ToolComplete`.
But wait — `TurnEvent::ToolComplete` currently only has `name: String, result: String`, no `is_error`. The design calls for `ToolEntry { name, params, result: Option<String>, is_error: bool, expanded: bool }`. The builder will need to track `is_error` — either from a changed `TurnEvent::ToolComplete` or by inferring it from ToolResult.

**Important constraint:** `TurnEvent::ToolComplete` currently has `result: String`. To get `is_error`, either:
1. Add `is_error: bool` to `TurnEvent::ToolComplete` (touches types.rs and turn.rs)
2. Infer from result string (fragile, as above)

The design (Section 3 data model) shows `ToolEntry.is_error` — most likely the builder adds `is_error: bool` to `TurnEvent::ToolComplete`. This is a Step 3 concern.
