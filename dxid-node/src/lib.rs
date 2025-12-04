use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use dxid_ai_hypervisor::Hypervisor;
use dxid_config::DxidConfig;
use dxid_consensus::{ConsensusConfig, HybridConsensus};
use dxid_core::{ChainState, TokenEconomics};
use dxid_crypto::DefaultCryptoProvider;
use dxid_network::{Libp2pNetwork, NetworkConfig as P2pConfig, NetworkService};
use dxid_rpc::start_servers;
use dxid_storage::PgStore;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

pub async fn run_node(config_path: PathBuf) -> Result<()> {
    let cfg = DxidConfig::load(&config_path)?;
    init_logging();
    info!("starting dxid node with config {:?}", config_path);
    let store = Arc::new(PgStore::connect(&cfg.db.url, cfg.db.pool_size).await?);
    let hypervisor = Arc::new(Hypervisor::new(cfg.ai.clone(), store.clone()));
    let crypto = Arc::new(DefaultCryptoProvider::new());
    let _consensus = Arc::new(HybridConsensus::new(
        crypto.clone(),
        ConsensusConfig {
            pow_target_spacing: 30,
            difficulty_window: 10,
            max_supply: cfg.consensus.max_supply,
            base_reward: cfg.consensus.base_reward,
        },
    ));

    let mut network = Libp2pNetwork::new(P2pConfig {
        listen_addr: cfg.network.listen_addr.clone(),
        seed_nodes: cfg.network.seed_nodes.clone(),
    })?;
    let network_task = tokio::spawn(async move { network.start().await });

    let rpc_task = tokio::spawn(start_servers(&cfg, store.clone(), hypervisor.clone()));

    // Join tasks
    network_task.await??;
    rpc_task.await??;
    Ok(())
}

fn init_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}
