use ap::app::AgentLoop;
use ap::config::AppConfig;
use ap::hooks::HookRunner;
use ap::provider::BedrockProvider;
use ap::session::{store::SessionStore, Session};
use ap::tools::ToolRegistry;
use ap::tui::TuiApp;
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

    // Load or create session
    let session: Option<Session> = match &args.session {
        Some(id) => {
            let store = SessionStore::new().unwrap_or_else(|e| {
                eprintln!("ap: warning: could not determine session dir: {e}");
                SessionStore::with_base(std::path::PathBuf::from(".ap/sessions"))
            });
            match store.load(id) {
                Ok(session) => {
                    eprintln!("ap: resuming session {id} ({} messages)", session.messages.len());
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

    match args.prompt {
        Some(prompt) => {
            // Non-interactive (headless) mode
            run_headless(config, session, &prompt).await
        }
        None => {
            // Interactive TUI mode
            run_tui(config, session).await
        }
    }
}

async fn run_headless(
    config: AppConfig,
    session: Option<Session>,
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

    // Build tools and hooks
    let tools = ToolRegistry::with_defaults();
    let hooks = HookRunner::new(config.hooks.clone());

    // Channel: agent → stdout sink
    let (ui_tx, mut ui_rx) = mpsc::channel(256);

    // Build agent loop (with session if provided)
    let mut agent = AgentLoop::with_session(provider, tools, hooks, ui_tx, session);

    // Spawn agent turn in background
    let prompt_owned = prompt.to_string();
    let agent_handle = tokio::spawn(async move { agent.run_turn(prompt_owned).await });

    // Drain events, printing text to stdout
    let mut exit_code = 0i32;
    let stdout = std::io::stdout();
    loop {
        match ui_rx.recv().await {
            Some(ap::app::UiEvent::TextChunk(text)) => {
                let mut out = stdout.lock();
                out.write_all(text.as_bytes()).ok();
                out.flush().ok();
            }
            Some(ap::app::UiEvent::ToolStart { name, .. }) => {
                eprintln!("ap: tool: {name}");
            }
            Some(ap::app::UiEvent::ToolComplete { name, result }) => {
                if result.is_error {
                    eprintln!("ap: tool {name} error: {}", result.content);
                }
            }
            Some(ap::app::UiEvent::TurnEnd) => {
                println!(); // final newline
                break;
            }
            Some(ap::app::UiEvent::Error(msg)) => {
                eprintln!("ap: error: {msg}");
                exit_code = 1;
                break;
            }
            None => {
                // Channel closed — agent finished
                break;
            }
        }
    }

    // Wait for agent task to complete
    if let Err(e) = agent_handle.await {
        eprintln!("ap: agent task panicked: {e}");
        exit_code = 1;
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
