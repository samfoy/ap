# Rough Idea: Skill System for `ap`

Offline, pure-Rust skill injection using TF-IDF relevance scoring.

Skills are markdown files in `~/.ap/skills/` or `./.ap/skills/` that are automatically
injected into the system prompt when relevant to the current conversation.

No external embedding APIs, no ML dependencies. Pure Rust, offline, fast.

See: /Users/sam.painter/Projects/ap/PROMPT.md for full spec.
