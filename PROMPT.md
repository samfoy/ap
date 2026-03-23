Now I have a thorough picture of the codebase. Let me write the PROMPT.md.Here is the `PROMPT.md` content written to disk:

---

## What's in the PROMPT

**Vision** — Model switching as a first-class runtime operation: `/model <id>` in the TUI, `--model` on the CLI, always visible in the status bar, remembered in `~/.ap/models.json`.

**Technical requirements** cover seven concrete areas with exact Rust types/signatures:

| Area | Key type/change |
|---|---|
| CLI flag | `Args.model: Option<String>`, applied before `build_provider` |
| Recent-models store | `RecentModels` in `src/models.rs` — `load()`, `load_from(path)`, `record(id)`, `save()` |
| `/model` command | `TuiApp::handle_model_switch` + `handle_model_query` in `handle_submit` |
| Immutable builder | `Conversation::with_model` |
| Provider contract | `stream_completion` gains `model_override: Option<&'a str>` — providers substitute it when `Some` |
| `turn_loop` wiring | passes `Some(conv.model.as_str())` so `conv.model` always wins |
| Context compression | passes `None` so the summariser never inherits the user-switched model |

**7 ordered steps**, each independently compilable:
1. `Conversation::with_model` + tests
2. `--model` CLI flag + clap parse tests
3. `RecentModels` store + round-trip tests
4. `Provider` trait `model_override` parameter — mechanical update of all `impl Provider` and call-sites
5. TUI `/model` command + `handle_model_switch` + async unit tests
6. Record model in headless + TUI startup (wires Step 3 into `main.rs`)
7. Status bar test + `ap.toml.example` documentation

**13 acceptance criteria** covering every observable behaviour, ending with:

> Output `LOOP_COMPLETE` when all acceptance criteria are met and the project builds clean.