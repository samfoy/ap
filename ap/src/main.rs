use ap::config::AppConfig;
use ap::session::{store::SessionStore, Session};
use clap::Parser;

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

fn main() {
    let args = Args::parse();
    // Load config (merge global + project); warn but don't exit on failure
    let config = AppConfig::load().unwrap_or_default();

    // Load or create session
    let _session: Option<Session> = match &args.session {
        Some(id) => {
            let store = SessionStore::new().unwrap_or_else(|e| {
                eprintln!("ap: warning: could not determine session dir: {e}");
                // Fall back to a no-op store in the current directory
                SessionStore::with_base(std::path::PathBuf::from(".ap/sessions"))
            });
            match store.load(id) {
                Ok(session) => {
                    eprintln!("ap: resuming session {id} ({} messages)", session.messages.len());
                    Some(session)
                }
                Err(e) => {
                    eprintln!("ap: warning: could not load session '{id}': {e}");
                    // Fall back to a new session with the given id
                    Some(Session::new(id.clone(), config.provider.model.clone()))
                }
            }
        }
        None => None,
    };

    // Mode dispatch will be implemented in subsequent tasks (TUI / non-interactive)
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
