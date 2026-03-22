# Broken Windows — richer-tui

## Broken Windows

### [ap/src/tui/mod.rs:192-197] Brittle is_error detection from result string
**Type**: complexity / magic-values
**Risk**: Low (fixed by Step 3 anyway)
**Fix**: Add `is_error: bool` to `TurnEvent::ToolComplete` and use it directly in `ToolEntry`
**Code**:
```rust
// current code
let icon = if result.starts_with("error") || result.contains("blocked") {
    "✗"
} else {
    "✓"
};
```

---

### [ap/src/tui/events.rs:30-39] Convoluted help-overlay dismissal logic
**Type**: complexity
**Risk**: Low
**Fix**: Simplify — check Ctrl+C first, then Esc, rather than nesting
**Code**:
```rust
// current code
if app.show_help {
    if key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Action::Quit;
        }
        app.show_help = false;
    }
    return Action::None;
}
```

---

### [ap/src/turn.rs:138] StreamEvent::TurnEnd discards token data
**Type**: dead-code (token data carried but unused)
**Risk**: Low (fixed by Step 1)
**Fix**: Capture `input_tokens` and `output_tokens` and emit `TurnEvent::Usage`
**Code**:
```rust
// current code
StreamEvent::TurnEnd { .. } => {
    conv = apply_post_turn(conv, middleware);
    break;
}
```

---

### [ap/src/tui/mod.rs:186] params displayed raw via Display on serde_json::Value
**Type**: formatting
**Risk**: Low
**Fix**: When replacing `tool_events: Vec<String>` with `ToolEntry` in Step 3, params are stored as `Value` not formatted eagerly — display can be truncated/pretty-printed in render
**Code**:
```rust
// current code
self.tool_events.push(format!("⟳ {name}({})", params));
```
