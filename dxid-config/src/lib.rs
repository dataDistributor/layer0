use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbConfig {
    pub url: String,
    pub pool_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub rest_addr: String,
    pub grpc_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub max_supply: u64,
    pub base_reward: u64,
    pub halving_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_addr: String,
    pub seed_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub openai_api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DxidConfig {
    pub db: DbConfig,
    pub api: ApiConfig,
    pub consensus: ConsensusConfig,
    pub network: NetworkConfig,
    pub ai: AiConfig,
}

impl DxidConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let builder = config::Config::builder()
            .add_source(config::File::from(path))
            .add_source(config::Environment::with_prefix("DXID").separator("__"));
        let cfg = builder.build()?;
        Ok(cfg.try_deserialize()?)
    }

    pub fn example() -> Self {
        Self {
            db: DbConfig {
                url: "postgres://user:password@localhost:5432/dxid".into(),
                pool_size: 5,
            },
            api: ApiConfig {
                rest_addr: "0.0.0.0:8080".into(),
                grpc_addr: "0.0.0.0:50051".into(),
            },
            consensus: ConsensusConfig {
                max_supply: 21_000_000_0000,
                base_reward: 50_0000,
                halving_interval: 100_000,
            },
            network: NetworkConfig {
                listen_addr: "/ip4/0.0.0.0/tcp/7000".into(),
                seed_nodes: vec![],
            },
            ai: AiConfig {
                openai_api_key: "set-me".into(),
                model: "gpt-4o-mini".into(),
            },
        }
    }
}
