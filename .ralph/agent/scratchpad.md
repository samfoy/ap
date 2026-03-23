# Model Switching — Scratchpad

## Iteration 1 — Design Start

### Codebase Analysis

**Config (`ap/src/config.rs`):**
- `ProviderConfig` already has a `model: String` field with default `"us.anthropic.claude-sonnet-4-6"`
- `overlay_from_table` already handles the `model` key from TOML
- Tests already verify `model` config loading
- The config field exists — just the default string may need updating to match the acceptance criteria string

**Provider (`ap/src/provider/`):**
- `Provider` trait: `stream_completion(&self, messages, tools, system_prompt) -> BoxStream`
- `BedrockProvider` stores `model: String` on the struct, clones it per call
- `Arc<dyn Provider>` is used everywhere — no `&mut self` possible without interior mutability

**TUI (`ap/src/tui/mod.rs`):**
- `TuiApp.model_name: String` already exists and is shown in status bar
- `handle_submit()` is where slash commands could be intercepted
- Provider is `Arc<dyn Provider>` shared with spawned tasks

**Main (`ap/src/main.rs`):**
- `Args` struct uses `clap::Parser` — easy to add `--model` flag
- Both `run_headless` and `run_tui` use `config.provider.model`

### Critical Design Decision

The big architectural question: how should `/model` propagate the new model to the provider?

Option A: **Per-call parameter** — add `model: &str` to `stream_completion()` signature. Clean, no interior mutability. `TuiApp.model_name` is the source of truth, passed down each turn.

Option B: **Interior mutability on provider** — `set_model(&self, model: String)` with `Mutex<String>` inside `BedrockProvider`. Works with `Arc<dyn Provider>` but adds shared-state complexity.

The prompt itself mentions both but doesn't specify. This is the key question to ask.

### Status

Asking Q1 about the provider mutation strategy.
