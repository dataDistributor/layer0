use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use dxid_ai_hypervisor::Hypervisor;
use dxid_config::DxidConfig;
use dxid_core::CrossChainMessage;
use dxid_node::run_node;
use dxid_wallet::WalletStore;
use tokio::runtime::Runtime;

#[derive(Parser)]
#[command(name = "dxid", version, about = "dxid Layer-0 CLI")]
struct Cli {
    /// If set, show help instead of launching TUI when no subcommand is provided.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    help_mode: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize config and genesis
    Init {
        #[arg(long, default_value = "config/dxid.toml")]
        config: PathBuf,
    },
    /// Start node
    Node {
        #[command(subcommand)]
        cmd: NodeCmd,
    },
    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        cmd: WalletCmd,
    },
    /// AI hypervisor query
    Ai {
        #[arg()]
        prompt: String,
    },
}

#[derive(Subcommand)]
enum NodeCmd {
    Start {
        #[arg(long, default_value = "config/dxid.toml")]
        config: PathBuf,
    },
    Status,
}

#[derive(Subcommand)]
enum WalletCmd {
    New {
        #[arg(long, default_value = "default")]
        name: String,
        #[arg(long)]
        password: String,
    },
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.command.is_none() && !cli.help_mode {
        return dxid_tui::launch_tui();
    }
    match cli.command.unwrap_or(Commands::Init {
        config: PathBuf::from("config/dxid.toml"),
    }) {
        Commands::Init { config } => init_config(config)?,
        Commands::Node { cmd } => match cmd {
            NodeCmd::Start { config } => {
                let rt = Runtime::new()?;
                rt.block_on(async move { run_node(config).await })?;
            }
            NodeCmd::Status => {
                println!("Status endpoint not implemented; query /status REST");
            }
        },
        Commands::Wallet { cmd } => match cmd {
            WalletCmd::New { name, password } => {
                let store = WalletStore::new(wallet_dir()?)?;
                let wallet = store.create(&name, &password)?;
                println!(
                    "Created wallet {} address {}",
                    wallet.name,
                    dxid_crypto::address_to_string(&wallet.address)
                );
            }
            WalletCmd::List => {
                let store = WalletStore::new(wallet_dir()?)?;
                for w in store.list()? {
                    println!(
                        "{} -> {}",
                        w.name,
                        dxid_crypto::address_to_string(&w.address)
                    );
                }
            }
        },
        Commands::Ai { prompt } => {
            let cfg = DxidConfig::example();
            let rt = Runtime::new()?;
            rt.block_on(async move {
                let store = Arc::new(dxid_storage::PgStore::connect(&cfg.db.url, cfg.db.pool_size).await?);
                let hypervisor = Hypervisor::new(cfg.ai.clone(), store);
                let ans = hypervisor.query(&prompt).await?;
                println!("{ans}");
                Ok::<(), anyhow::Error>(())
            })?;
        }
    }
    Ok(())
}

fn init_config(path: PathBuf) -> Result<()> {
    if path.exists() {
        println!("Config already exists at {:?}", path);
        return Ok(());
    }
    let cfg = DxidConfig::example();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, toml::to_string_pretty(&cfg)?)?;
    println!("Wrote config to {:?}", path);
    Ok(())
}

fn wallet_dir() -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .unwrap_or(std::env::temp_dir())
        .join(".dxid")
        .join("wallets");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
