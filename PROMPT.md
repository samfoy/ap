Now I have a thorough understanding of the codebase. Let me write the PROMPT.md.Here is the complete `PROMPT.md` content that was written:

---

The prompt covers:

**Vision** — clear statement that this is a drop-in backend swap via config only, no recompilation.

**Technical requirements** with exact Rust types:
- `ProviderConfig` extended with `base_url` and `api_key`
- Full `OpenAiCompatProvider` struct signature with `OpenAiStreamState` state machine
- Precise SSE parsing rules for text deltas, tool call accumulation across chunks, index-change detection, `[DONE]` handling, and token extraction
- `build_messages` conversion table covering all four `MessageContent` variants including mixed assistant messages
- `build_tools` schema format translation (Anthropic `input_schema` → OpenAI `parameters`)
- `build_provider` factory function signature

**8 ordered implementation steps**, each independently compilable:
1. Extend `ProviderConfig` with new fields + overlay logic
2. Skeleton `OpenAiCompatProvider` (compiles, stub impl)
3. SSE parsing: text delta + `[DONE]`
4. SSE parsing: tool call accumulation + index transitions
5. Token count extraction into `OpenAiStreamState`
6. `build_messages` / `build_tools` format conversion
7. Real HTTP streaming in `stream_completion`
8. `build_provider` factory + `main.rs` wiring

**12 acceptance criteria** covering compilation cleanliness, all test suites, every new type/function, `turn()` immutability, and the example config file.