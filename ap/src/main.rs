#![deny(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
use ap::config::AppConfig;
use ap::middleware::shell_hook_bridge;
use ap::provider::BedrockProvider;
use ap::session::{store::SessionStore, Session};
use ap::tools::ToolRegistry;
use ap::tui::TuiApp;
use ap::turn::turn;
use ap::types::{Conversation, TurnEvent};
use clap::Parser;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    // Load config (merge global + project); warn but don't exit on failure
    let config = AppConfig::load().unwrap_or_default();

    if let Some(prompt) = args.prompt {
        // Non-interactive (headless) mode — session handled inside run_headless
        run_headless(config, args.session, &prompt).await
    } else {
        // Interactive TUI mode — load session here for the TUI path
        let session: Option<Session> = match &args.session {
            Some(id) => {
                let store = SessionStore::new().unwrap_or_else(|e| {
                    eprintln!("ap: warning: could not determine session dir: {e}");
                    SessionStore::with_base(std::path::PathBuf::from(".ap/sessions"))
                });
                match store.load(id) {
                    Ok(session) => {
                        eprintln!(
                            "ap: resuming session {id} ({} messages)",
                            session.messages.len()
                        );
                        Some(session)
                    }
                    Err(e) => {
                        eprintln!("ap: warning: could not load session '{id}': {e}");
                        Some(Session::new(id.clone(), config.provider.model.clone()))
                    }
                }
            }
            None => None,
        };
        run_tui(config, session).await
    }
}

async fn run_headless(
    config: AppConfig,
    session_id: Option<String>,
    prompt: &str,
) -> anyhow::Result<()> {
    use std::io::Write;

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

    // Build tools (recipe-style)
    let tools = ToolRegistry::with_defaults();

    // Build middleware from shell hooks config
    let middleware = shell_hook_bridge(&config.hooks);

    // Set up session store (only when --session is given)
    let store: Option<SessionStore> = session_id.as_ref().map(|_| {
        SessionStore::new().unwrap_or_else(|e| {
            eprintln!("ap: warning: could not determine session dir: {e}");
            SessionStore::with_base(std::path::PathBuf::from(".ap/sessions"))
        })
    });

    // Load or create the Conversation
    let conv: Conversation = match (&session_id, &store) {
        (Some(id), Some(s)) => match s.load_conversation(id) {
            Ok(c) => {
                eprintln!("ap: resuming session {id} ({} messages)", c.messages.len());
                c
            }
            Err(_) => {
                Conversation::new(id.clone(), config.provider.model.clone(), config.clone())
            }
        },
        _ => Conversation::new("ephemeral", config.provider.model.clone(), config.clone()),
    };

    // Run turn() — pure function, returns (updated_conv, events)
    let conv_with_msg = conv.with_user_message(prompt.to_string());
    let (updated_conv, events) =
        match turn(conv_with_msg, provider.as_ref(), &tools, &middleware).await {
            Ok(result) => result,
            Err(e) => {
                eprintln!("ap: error: {e}");
                std::process::exit(1);
            }
        };

    // Route events to stdout/stderr
    let stdout = std::io::stdout();
    let mut exit_code = 0i32;
    for event in &events {
        match event {
            TurnEvent::TextChunk(text) => {
                let mut out = stdout.lock();
                out.write_all(text.as_bytes()).ok();
                out.flush().ok();
            }
            TurnEvent::ToolStart { name, .. } => {
                eprintln!("ap: tool: {name}");
            }
            TurnEvent::ToolComplete { .. } | TurnEvent::Usage { .. } => {
                // ToolComplete: results shown in context; errors surfaced via Error event
                // Usage: displayed in TUI status bar; headless mode ignores it
            }
            TurnEvent::TurnEnd => {
                println!(); // final newline
            }
            TurnEvent::Error(msg) => {
                eprintln!("ap: error: {msg}");
                exit_code = 1;
            }
        }
    }

    // Save conversation if session was requested and turn succeeded
    if exit_code == 0 {
        if let (Some(_id), Some(s)) = (&session_id, &store) {
            if let Err(e) = s.save_conversation(&updated_conv) {
                eprintln!("ap: warning: could not save session: {e}");
            }
        }
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

async fn run_tui(config: AppConfig, _session: Option<Session>) -> anyhow::Result<()> {
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

    // Build tools, middleware, and conversation
    let tools = Arc::new(ToolRegistry::with_defaults());
    let middleware = Arc::new(shell_hook_bridge(&config.hooks));
    let model_name = config.provider.model.clone();
    let conv = Arc::new(tokio::sync::Mutex::new(Conversation::new(
        uuid::Uuid::new_v4().to_string(),
        model_name.clone(),
        config.clone(),
    )));

    // Build and run TUI
    let mut app = TuiApp::new(conv, provider, tools, middleware, model_name)?;
    app.run().await
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
