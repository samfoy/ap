mod config;
pub mod provider;
pub mod tools;

use clap::Parser;
use config::AppConfig;

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
    let _args = Args::parse();
    // Load config (merge global + project); warn but don't exit on failure
    let _config = AppConfig::load().unwrap_or_default();
    // Mode dispatch will be implemented in subsequent tasks
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_version_string() {
        // The version is set in Cargo.toml as "0.1.0"
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0");
    }

    #[test]
    fn test_binary_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "ap");
    }
}
