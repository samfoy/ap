# PROMPT.md — Model Switching

## Vision

Users can swap the active AI model mid-session without restarting `ap`. A `/model <name>` slash command switches the model for all subsequent turns. The current model is shown in the TUI status bar and in `--prompt` mode output. Config supports a default model via `[provider] model = "..."` in `~/.ap/config.toml`.

## Requirements

### 1. Config: default model
- Add `model` field to `[provider]` section in `Config` struct (`src/config.rs`)
- Default: `"us.anthropic.claude-sonnet-4-6-v1:0"` (current hardcoded value)
- Read from `~/.ap/config.toml` on startup

### 2. Runtime model switching
- Add `/model <name>` slash command handler in `handle_submit` (or a dedicated slash command parser)
- Switching model updates `TuiApp.model_name` and passes new model to the provider for subsequent turns
- Print confirmation inline: `Model switched to: <name>`
- Invalid model names: show error inline, don't crash

### 3. Provider accepts model per-turn
- `BedrockProvider` currently hardcodes the model — refactor to accept `model: &str` as a parameter on `complete()` / streaming call
- Or store on the provider struct and expose a `set_model(&mut self, model: String)` method

### 4. Status bar shows current model
- Already shows model_name — ensure it updates immediately after `/model` switch (no restart needed)

### 5. `--prompt` mode respects config model
- When running `ap --prompt "..."`, use model from config (or `--model` CLI flag if provided)
- Add optional `--model <name>` CLI flag to override config for a single run

## Acceptance Criteria

- `ap` starts with model from `~/.ap/config.toml` (falls back to default if not set)
- `/model claude-sonnet-4-5` switches model for next turn, status bar updates
- `ap --model us.anthropic.claude-haiku-3-5-v1:0 --prompt "hello"` uses specified model
- `cargo build` passes
- `cargo test` passes (≥204 tests)

Output LOOP_COMPLETE when all acceptance criteria are met and the project builds clean.
