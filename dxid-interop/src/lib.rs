use anyhow::Result;
use async_trait::async_trait;
use dxid_core::{ChainMetadata, CrossChainMessage};
use dxid_crypto::{Groth16Backend, StarkProofWrapper, WinterfellBackend, ZkSnarkBackend, ZkStarkBackend};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalChainConfig {
    pub name: String,
    pub rpc_endpoint: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalChainHandle {
    pub id: Uuid,
    pub metadata: ChainMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxReceipt {
    pub id: Uuid,
    pub accepted: bool,
    pub response: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalStateQuery {
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalStateResponse {
    pub result: Value,
}

#[derive(Debug, Error)]
pub enum InteropError {
    #[error("http error: {0}")]
    Http(String),
    #[error("proof error: {0}")]
    Proof(String),
    #[error("other: {0}")]
    Other(String),
}

#[async_trait]
pub trait ChainAdapter: Send + Sync {
    async fn connect(&self, config: &ExternalChainConfig) -> Result<ExternalChainHandle, InteropError>;
    async fn send_message(
        &self,
        proof: &dxid_crypto::SnarkProof,
        msg: &CrossChainMessage,
    ) -> Result<TxReceipt, InteropError>;
    async fn query_state(&self, query: &ExternalStateQuery) -> Result<ExternalStateResponse, InteropError>;
}

pub struct HttpJsonRpcAdapter {
    client: Client,
    stark: Box<dyn ZkStarkBackend>,
    snark: Box<dyn ZkSnarkBackend>,
}

impl HttpJsonRpcAdapter {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            stark: Box::new(WinterfellBackend::new()),
            snark: Box::new(Groth16Backend::new().expect("groth16 backend")),
        }
    }
}

#[async_trait]
impl ChainAdapter for HttpJsonRpcAdapter {
    async fn connect(&self, config: &ExternalChainConfig) -> Result<ExternalChainHandle, InteropError> {
        let metadata = ChainMetadata {
            chain_id: config.name.clone(),
            rpc_endpoint: config.rpc_endpoint.clone(),
            latest_height: 0,
            network: "external".into(),
            extra: config.metadata.clone(),
        };
        let proof = self
            .stark
            .prove_connection(&metadata)
            .map_err(|e| InteropError::Proof(e.to_string()))?;
        self.stark
            .verify_connection(&proof, &metadata)
            .map_err(|e| InteropError::Proof(e.to_string()))?;
        Ok(ExternalChainHandle {
            id: Uuid::new_v4(),
            metadata,
        })
    }

    async fn send_message(
        &self,
        proof: &dxid_crypto::SnarkProof,
        msg: &CrossChainMessage,
    ) -> Result<TxReceipt, InteropError> {
        self.snark
            .verify_message(proof, msg)
            .map_err(|e| InteropError::Proof(e.to_string()))?;
        let resp = self
            .client
            .post(msg.dest.clone())
            .json(&serde_json::json!({
                "method": "dxid_bridge",
                "params": msg,
                "proof": proof
            }))
            .send()
            .await
            .map_err(|e| InteropError::Http(e.to_string()))?;
        let body = resp
            .json::<Value>()
            .await
            .map_err(|e| InteropError::Http(e.to_string()))?;
        Ok(TxReceipt {
            id: msg.id,
            accepted: true,
            response: body,
        })
    }

    async fn query_state(&self, query: &ExternalStateQuery) -> Result<ExternalStateResponse, InteropError> {
        let resp = self
            .client
            .post("http://placeholder-rpc")
            .json(&serde_json::json!({
                "method": query.method,
                "params": query.params
            }))
            .send()
            .await
            .map_err(|e| InteropError::Http(e.to_string()))?;
        let val = resp
            .json::<Value>()
            .await
            .map_err(|e| InteropError::Http(e.to_string()))?;
        Ok(ExternalStateResponse { result: val })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn proof_roundtrip() {
        let adapter = HttpJsonRpcAdapter::new();
        let cfg = ExternalChainConfig {
            name: "demo".into(),
            rpc_endpoint: "http://localhost:8545".into(),
            metadata: serde_json::json!({}),
        };
        let handle = adapter.connect(&cfg).await.unwrap();
        let msg = CrossChainMessage {
            id: Uuid::new_v4(),
            source: "demo".into(),
            dest: "http://localhost:8545".into(),
            payload: serde_json::json!({"ping": true}),
            nonce: 1,
            timestamp: 0,
        };
        let proof = adapter.snark.prove_message(&msg).unwrap();
        // verify_message is called inside send_message; invoke directly for test
        adapter.snark.verify_message(&proof, &msg).unwrap();
    }
}
