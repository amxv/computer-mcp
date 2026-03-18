use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use computer_mcp::config::{Config, DEFAULT_CONFIG_PATH};
use rand::distr::{Alphanumeric, SampleString};

#[derive(Debug, Parser)]
#[command(name = "computer-mcp")]
#[command(about = "Computer MCP CLI")]
struct Cli {
    #[arg(long, default_value = DEFAULT_CONFIG_PATH)]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install,
    Start,
    Stop,
    Restart,
    Status,
    Logs,
    SetKey {
        value: String,
    },
    RotateKey,
    ShowUrl {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    Tls {
        #[command(subcommand)]
        command: TlsCommand,
    },
}

#[derive(Debug, Subcommand)]
enum TlsCommand {
    Setup,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = PathBuf::from(&cli.config);

    match cli.command {
        Commands::Install => {
            println!("install scaffold ready: systemd setup is not implemented in this slice");
        }
        Commands::Start => {
            let status = Command::new("computer-mcpd")
                .arg("--config")
                .arg(&cli.config)
                .status()
                .context("failed to run computer-mcpd")?;
            if !status.success() {
                anyhow::bail!("computer-mcpd exited with status: {status}");
            }
        }
        Commands::Stop => {
            println!("stop scaffold ready: service management will be added in next phase");
        }
        Commands::Restart => {
            println!("restart scaffold ready: service management will be added in next phase");
        }
        Commands::Status => {
            println!("status scaffold ready: service status checks will be added in next phase");
        }
        Commands::Logs => {
            println!(
                "logs scaffold ready: journald/systemd integration will be added in next phase"
            );
        }
        Commands::SetKey { value } => {
            let mut config = Config::load(Some(Path::new(&config_path)))?;
            config.api_key = value;
            config.save(&config_path)?;
            println!("updated API key in {}", config_path.display());
        }
        Commands::RotateKey => {
            let mut config = Config::load(Some(Path::new(&config_path)))?;
            let mut rng = rand::rng();
            config.api_key = Alphanumeric.sample_string(&mut rng, 48);
            config.save(&config_path)?;
            println!("rotated API key in {}", config_path.display());
        }
        Commands::ShowUrl { host } => {
            let config = Config::load(Some(Path::new(&config_path)))?;
            println!("https://{host}/mcp?key={}", config.api_key);
        }
        Commands::Tls { command } => match command {
            TlsCommand::Setup => {
                println!("tls setup scaffold ready: LE IP + self-signed fallback is pending");
            }
        },
    }

    Ok(())
}
