use ap::app::AgentLoop;
use ap::config::AppConfig;
use ap::hooks::HookRunner;
use ap::middleware::shell_hook_bridge;
use ap::provider::BedrockProvider;
use ap::session::{store::SessionStore, Session};
use ap::tools::ToolRegistry;
use ap::tui::TuiApp;
use ap::turn::turn;
use ap::types::{Conversation, TurnEvent};
use clap::Parser;
use tokio::sync::mpsc;

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

    match args.prompt {
        Some(prompt) => {
            // Non-interactive (headless) mode — session handled inside run_headless
            run_headless(config, args.session, &prompt).await
        }
        None => {
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
}

async fn run_headless(
    config: AppConfig,
    session_id: Option<String>,
    prompt: &str,
) -> anyhow::Result<()> {
    use std::io::Write;
    use std::sync::Arc;

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

    // Channel: turn() → stdout sink
    let (tx, mut rx) = mpsc::channel::<TurnEvent>(256);

    // Run turn() in a background task so we can drain the channel concurrently
    let tx_for_turn = tx.clone();
    let prompt_owned = prompt.to_string();
    let conv_with_msg = conv.with_user_message(prompt_owned);
    let turn_handle = tokio::spawn(async move {
        turn(conv_with_msg, provider.as_ref(), &tools, &middleware, &tx_for_turn).await
    });
    drop(tx); // drop original sender; rx gets None when turn finishes

    // Drain events, printing text to stdout
    let mut exit_code = 0i32;
    let stdout = std::io::stdout();
    loop {
        match rx.recv().await {
            Some(TurnEvent::TextChunk(text)) => {
                let mut out = stdout.lock();
                out.write_all(text.as_bytes()).ok();
                out.flush().ok();
            }
            Some(TurnEvent::ToolStart { name, .. }) => {
                eprintln!("ap: tool: {name}");
            }
            Some(TurnEvent::ToolComplete { name, result }) => {
                // Only surface errors — successful results are shown in context
                let _ = name;
                let _ = result;
            }
            Some(TurnEvent::TurnEnd) => {
                println!(); // final newline
                break;
            }
            Some(TurnEvent::Error(msg)) => {
                eprintln!("ap: error: {msg}");
                exit_code = 1;
                break;
            }
            None => {
                // Channel closed — turn finished
                break;
            }
        }
    }

    // Wait for the turn task and handle its result
    let updated_conv = match turn_handle.await {
        Ok(Ok(c)) => Some(c),
        Ok(Err(e)) => {
            eprintln!("ap: error: {e}");
            exit_code = 1;
            None
        }
        Err(e) => {
            eprintln!("ap: agent task panicked: {e}");
            exit_code = 1;
            None
        }
    };

    // Save conversation if session was requested and turn succeeded
    if let (Some(id), Some(s), Some(conv)) = (&session_id, &store, updated_conv) {
        let _ = id;
        if let Err(e) = s.save_conversation(&conv) {
            eprintln!("ap: warning: could not save session: {e}");
        }
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

async fn run_tui(config: AppConfig, session: Option<Session>) -> anyhow::Result<()> {
    use std::sync::Arc;

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

    // Build tools and hooks
    let tools = ToolRegistry::with_defaults();
    let hooks = HookRunner::new(config.hooks.clone());

    // Channel: agent → TUI
    let (ui_tx, ui_rx) = mpsc::channel(256);

    // Build agent loop (with session if provided)
    let agent = AgentLoop::with_session(provider, tools, hooks, ui_tx, session);

    let model_name = config.provider.model.clone();

    // Build and run TUI
    let mut app = TuiApp::new(ui_rx, agent, model_name)?;
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
