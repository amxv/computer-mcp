use std::path::Path;

use anyhow::Result;
use clap::Parser;
use computer_mcp::config::{Config, DEFAULT_CONFIG_PATH};
use computer_mcp::server::run_server;
use tracing::warn;

#[derive(Debug, Parser)]
#[command(name = "computer-mcpd")]
#[command(about = "Computer MCP daemon")]
struct Args {
    #[arg(long, default_value = DEFAULT_CONFIG_PATH)]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "computer_mcp=info,computer_mcpd=info".to_string()),
        )
        .init();

    let args = Args::parse();
    let config = Config::load(Some(Path::new(&args.config)))?;

    warn!(
        "computer-mcpd exposes high-privilege remote execution; protect API keys and network access"
    );

    run_server(config).await
}
