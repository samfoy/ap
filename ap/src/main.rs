#![deny(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
use ap::config::AppConfig;
use ap::context::maybe_compress_context;
use ap::discovery::discover;
use ap::middleware::shell_hook_bridge;
use ap::provider::BedrockProvider;
use ap::session::store::SessionStore;
use ap::skills::{skill_injection_middleware, SkillLoader};
use ap::tools::{ShellTool, ToolRegistry};
use ap::tui::TuiApp;
use ap::turn::turn;
use ap::types::{Conversation, TurnEvent};
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;

/// ap — A terminal AI coding agent powered by AWS Bedrock
#[derive(Parser, Debug)]
#[command(name = "ap", version = "0.1.0", about = "A terminal AI coding agent")]
struct Args {
    /// Run in non-interactive mode with a prompt
    #[arg(short = 'p', long = "prompt")]
    prompt: Option<String>,

    /// Resume a previous session by ID
    #[arg(short = 's', long = "session")]
    session: Option<String>,

    /// Override the context limit (in tokens) from the config file
    #[arg(long)]
    context_limit: Option<u32>,

    /// List saved sessions and exit
    #[arg(long = "list-sessions")]
    list_sessions: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    // Load config (merge global + project); warn but don't exit on failure
    let mut config = AppConfig::load().unwrap_or_default();

    // CLI flag overrides config file value
    if let Some(limit) = args.context_limit {
        config.context.limit = Some(limit);
    }

    // --list-sessions: print all saved sessions and exit
    if args.list_sessions {
        let store = SessionStore::new()?;
        let sessions = store.list()?;
        for s in &sessions {
            let date = s.created_at.get(..10).unwrap_or(&s.created_at);
            println!("{:<30} {}  {} messages", s.name, date, s.message_count);
        }
        return Ok(());
    }

    if let Some(prompt) = args.prompt {
        // Non-interactive (headless) mode — session handled inside run_headless
        run_headless(config, args.session, &prompt).await
    } else {
        // Interactive TUI mode — session init handled inside run_tui
        run_tui(config, args.session).await
    }
}

#[allow(clippy::too_many_lines)]
async fn run_headless(
    config: AppConfig,
    session_id: Option<String>,
    prompt: &str,
) -> anyhow::Result<()> {
    // Build the Bedrock provider
    let provider = match BedrockProvider::new(
        config.provider.model.clone(),
        config.provider.region.clone(),
    )
    .await
    {
        Ok(p) => Arc::new(p) as Arc<dyn ap::provider::Provider>,
        Err(e) => {
            eprintln!("ap: failed to initialise Bedrock provider: {e}");
            std::process::exit(1);
        }
    };

    // Discover project tools and skill system prompts
    let project_root =
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let discovery = discover(&project_root);
    for w in &discovery.warnings {
        eprintln!("ap: {w}");
    }

    // Build tools (recipe-style) — register ShellTools before any Arc wrap
    let mut tools = ToolRegistry::with_defaults();
    for discovered in discovery.tools {
        tools.register(Box::new(ShellTool::new(discovered, project_root.clone())));
    }

    // Resolve skill directories
    let skill_dirs = resolve_skill_dirs(config.skills.dirs.as_ref());

    // Build middleware from shell hooks config + optional skill injection
    let middleware = {
        let mw = shell_hook_bridge(&config.hooks);
        if config.skills.enabled {
            let loader = SkillLoader::new(skill_dirs);
            mw.pre_turn(skill_injection_middleware(loader, config.skills.clone()))
        } else {
            mw
        }
    };

    // Set up session store — always created; fall back to a local path on error
    let store: Option<SessionStore> = match SessionStore::new() {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("ap: warning: could not initialise session store: {e}");
            None
        }
    };

    // Resolve session name: use --session value or generate a fresh adjective-noun name
    let session_name: String =
        session_id.unwrap_or_else(SessionStore::generate_name);

    // Load prior messages for this session (empty vec if new session or no store)
    let prior_messages = store
        .as_ref()
        .and_then(|s| s.load(&session_name).ok())
        .unwrap_or_default();
    if !prior_messages.is_empty() {
        eprintln!(
            "ap: resuming session {} ({} messages)",
            session_name,
            prior_messages.len()
        );
    }

    // Load or create the Conversation with prior history
    let conv: Conversation = Conversation::new(
        session_name.clone(),
        config.provider.model.clone(),
        config.clone(),
    )
    .with_messages(prior_messages);

    // Apply discovered system prompt additions
    let system_prompt: Option<String> = if discovery.system_prompt_additions.is_empty() {
        None
    } else {
        Some(discovery.system_prompt_additions.join("\n\n"))
    };
    let conv = match system_prompt {
        Some(sp) => conv.with_system_prompt(sp),
        None => conv,
    };

    // Run turn() — pure function, returns (updated_conv, events)
    let conv_with_msg = conv.with_user_message(prompt.to_string());

    // Conditionally compress context before turn(). Clone first so we have a
    // fallback if maybe_compress_context returns Err (ownership is moved below).
    let conv_to_use = if config.context.limit.is_some() {
        let fallback = conv_with_msg.clone();
        match maybe_compress_context(conv_with_msg, &config.context, provider.as_ref()).await {
            Ok((c, Some(TurnEvent::ContextSummarized {
                messages_before,
                messages_after,
                tokens_before,
                tokens_after,
            }))) => {
                eprintln!(
                    "ap: context summarized: {messages_before}→{messages_after} messages, \
                     {tokens_before}→{tokens_after} tokens"
                );
                c
            }
            Ok((c, _)) => c,
            Err(e) => {
                eprintln!("ap: warn: context compression failed: {e}");
                fallback
            }
        }
    } else {
        conv_with_msg
    };

    let (updated_conv, events) =
        match turn(conv_to_use, provider.as_ref(), &tools, &middleware).await {
            Ok(result) => result,
            Err(e) => {
                eprintln!("ap: error: {e}");
                std::process::exit(1);
            }
        };

    // Route events to stdout/stderr; returns non-zero on error
    let exit_code = route_headless_events(&events);

    // Always save the session (if store is available and turn succeeded)
    if exit_code == 0 {
        if let Some(s) = &store {
            match s.save(&session_name, &updated_conv) {
                Ok(()) => eprintln!("Session saved: {session_name}"),
                Err(e) => eprintln!("ap: warning: could not save session: {e}"),
            }
        }
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

/// Stream `events` to stdout/stderr and return an exit code (0 = success, 1 = error).
fn route_headless_events(events: &[TurnEvent]) -> i32 {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut exit_code = 0i32;
    for event in events {
        match event {
            TurnEvent::TextChunk(text) => {
                let mut out = stdout.lock();
                out.write_all(text.as_bytes()).ok();
                out.flush().ok();
            }
            TurnEvent::ToolStart { name, .. } => eprintln!("ap: tool: {name}"),
            TurnEvent::ToolComplete { .. } | TurnEvent::Usage { .. } => {
                // ToolComplete: results shown in context; errors surfaced via Error event
                // Usage: displayed in TUI status bar; headless mode ignores it
            }
            TurnEvent::TurnEnd => println!(),
            TurnEvent::Error(msg) => {
                eprintln!("ap: error: {msg}");
                exit_code = 1;
            }
            TurnEvent::ContextSummarized { .. } => {
                eprintln!("context summarized");
            }
        }
    }
    exit_code
}

async fn run_tui(config: AppConfig, session_arg: Option<String>) -> anyhow::Result<()> {
    // ── Session init ──────────────────────────────────────────────────────────
    // Mirror the headless path: always create a store, resolve a name, load history.
    let store: Option<SessionStore> = match SessionStore::new() {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("ap: warning: cannot initialise session store: {e}");
            None
        }
    };

    // Resolve session name: use --session value or generate a fresh adjective-noun name
    let session_name: String = session_arg.unwrap_or_else(SessionStore::generate_name);

    // Load prior messages for this session (empty vec if new or no store)
    let prior_messages = store
        .as_ref()
        .and_then(|s| s.load(&session_name).ok())
        .unwrap_or_default();
    if !prior_messages.is_empty() {
        eprintln!(
            "ap: resuming session {} ({} messages)",
            session_name,
            prior_messages.len()
        );
    }

    // ── Provider ──────────────────────────────────────────────────────────────
    let provider = match BedrockProvider::new(
        config.provider.model.clone(),
        config.provider.region.clone(),
    )
    .await
    {
        Ok(p) => Arc::new(p) as Arc<dyn ap::provider::Provider>,
        Err(e) => {
            eprintln!("ap: failed to initialise Bedrock provider: {e}");
            std::process::exit(1);
        }
    };

    // ── Discover project tools and skill system prompts ───────────────────────
    let project_root =
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let discovery = discover(&project_root);
    for w in &discovery.warnings {
        eprintln!("ap: {w}");
    }

    // ── Tools & middleware ────────────────────────────────────────────────────
    // ShellTools must be registered BEFORE Arc::new wrap
    let mut tools = ToolRegistry::with_defaults();
    for discovered in discovery.tools {
        tools.register(Box::new(ShellTool::new(discovered, project_root.clone())));
    }
    let tools = Arc::new(tools);
    let middleware = {
        let skill_dirs = resolve_skill_dirs(config.skills.dirs.as_ref());
        let mw = shell_hook_bridge(&config.hooks);
        if config.skills.enabled {
            let loader = SkillLoader::new(skill_dirs);
            mw.pre_turn(skill_injection_middleware(loader, config.skills.clone()))
        } else {
            mw
        }
    };
    let middleware = Arc::new(middleware);
    let model_name = config.provider.model.clone();

    // ── Initial conversation with loaded history ───────────────────────────────
    // Apply discovered system prompt additions
    let system_prompt: Option<String> = if discovery.system_prompt_additions.is_empty() {
        None
    } else {
        Some(discovery.system_prompt_additions.join("\n\n"))
    };
    let base_conv = Conversation::new(
        session_name.clone(),
        model_name.clone(),
        config.clone(),
    )
    .with_messages(prior_messages);
    let initial_conv = match system_prompt {
        Some(sp) => base_conv.with_system_prompt(sp),
        None => base_conv,
    };
    let conv = Arc::new(tokio::sync::Mutex::new(initial_conv));

    // ── Build and run TUI, passing session context for auto-save ─────────────
    let store_arc = store.map(Arc::new);
    let mut app = TuiApp::new(
        conv,
        provider,
        tools,
        middleware,
        model_name,
        config.context.limit,
        Some(session_name),
        store_arc,
    )?;
    app.run().await
}

/// Resolve the skill directories to pass to `SkillLoader`.
///
/// If `dirs` is `Some`, use those directly.
/// Otherwise build the default list: `~/.ap/skills/` and `./.ap/skills/`,
/// filtering to paths that currently exist on disk.
fn resolve_skill_dirs(dirs: Option<&Vec<PathBuf>>) -> Vec<PathBuf> {
    if let Some(explicit) = dirs {
        return explicit.clone();
    }
    let mut default_dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        default_dirs.push(home.join(".ap/skills"));
    }
    default_dirs.push(PathBuf::from(".ap/skills"));
    default_dirs.into_iter().filter(|p| p.exists()).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_version_string() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0");
    }

    #[test]
    fn test_binary_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "ap");
    }
}
