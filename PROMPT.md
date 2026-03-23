Now I have a thorough understanding of the codebase. Let me write the PROMPT.md:Here is the `PROMPT.md` content written to `/Users/sam.painter/Projects/ap/ap/PROMPT.md`:

---

The prompt is structured around **7 independently-compilable steps**, each building on the last:

| Step | What it adds |
|------|-------------|
| 1 | `SessionStore::list()` + `SessionMeta` — pure data, no CLI changes |
| 2 | `slug_from_message()` in `session/slug.rs` — deterministic, fully testable |
| 3 | Always-on persistence in `run_headless()` + `--resume`/`--fork` CLI flags |
| 4 | `ap sessions` subcommand with formatted table output |
| 5 | Always-on persistence in `run_tui()`, `TurnEvent::SessionSaved`, status bar |
| 6 | TUI session browser overlay (`s` key, preview pane, Enter/f/Esc) |
| 7 | `--resume` / `--fork` wiring into TUI mode + `fuzzy_find()` |

Key design decisions captured in the prompt:
- **`Conversation.id` is the session name** — no new field, slug becomes the id on first save
- **"pending-`<uuid>`" placeholder** until the first message arrives in TUI mode, then replaced with the slug
- **`SessionMeta` is derived from on-disk `Conversation` files** (not the legacy `Session` struct) so it stays consistent with what `save_conversation` actually writes
- All new code must pass the project's existing `#![deny(clippy::unwrap_used)]` etc. lints