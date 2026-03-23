Now I have a thorough understanding of the codebase. Let me write the PROMPT.md:Here is the `PROMPT.md` content written to `~/Projects/ap/ap/PROMPT.md`:

---

## What the document covers

**Vision** — Model switching without restart: `/model <id>` in the TUI, `--model` at the CLI, model always visible in the status bar, recent models remembered in `~/.ap/models.json`.

**Precise types/signatures** — Every new type, every changed signature, and every new enum variant is specified verbatim in Rust, matched exactly to the existing codebase (e.g. `Action`, `TuiApp`, `Args`, `AppConfig`, `BedrockProvider`).

**Six independently compilable steps:**

| Step | Scope | Key deliverable |
|------|-------|-----------------|
| 1 | `src/models.rs` | Pure `RecentModels` struct + file I/O, 8 tests |
| 2 | `src/main.rs` | `--model`/`-m` CLI flag, 3 tests |
| 3 | `src/tui/events.rs` | `Action::ModelSwitch`, intercept `/model` on Enter, 6 tests |
| 4 | `src/tui/mod.rs` | `handle_model_switch` — hot-swap provider, update conv, save recents, 4 tests |
| 5 | `src/tui/ui.rs` | `format_model_segment` truncation helper, 5 tests |
| 6 | Wiring + integration | `--model` flows through both modes, final clean build |

**15 acceptance criteria** — each directly verifiable by `cargo test` or CLI invocation, with no ambiguity about pass/fail.

**Ralph-specific notes** — clippy deny rules, `async` mutex locking discipline, no new `rand` dep, test strategy for `BedrockProvider::new` in CI.