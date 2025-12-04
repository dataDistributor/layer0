use anyhow::Result;
use dxid_core::IdentityId;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EmbeddingId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    pub id: EmbeddingId,
    pub namespace: String,
    pub values: Vec<f32>,
    pub metadata: Value,
}

impl Embedding {
    pub fn new(namespace: String, values: Vec<f32>, metadata: Value) -> Self {
        let id = EmbeddingId(Uuid::new_v4().to_string());
        Self {
            id,
            namespace,
            values,
            metadata,
        }
    }
}

pub fn embed_identity_metadata(identity: &IdentityId, attrs: &[(String, String)]) -> Embedding {
    let mut acc: f32 = 0.0;
    for (k, v) in attrs {
        acc += (k.len() + v.len()) as f32;
    }
    let values = vec![acc, acc / 2.0, acc / 3.0];
    Embedding::new(
        format!("identity:{}", identity),
        values,
        serde_json::json!({ "attributes": attrs }),
    )
}

pub fn embed_chain_state(height: u64, peers: usize) -> Embedding {
    let values = vec![height as f32, peers as f32, (height % 10) as f32];
    Embedding::new(
        "chain:state".to_string(),
        values,
        serde_json::json!({ "height": height, "peers": peers }),
    )
}

pub fn random_vector(dim: usize) -> Vec<f32> {
    let mut rng = rand::thread_rng();
    (0..dim).map(|_| rng.gen::<f32>()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_embedding() {
        let id = IdentityId::new_v4();
        let emb = embed_identity_metadata(&id, &[("role".into(), "admin".into())]);
        assert!(emb.values.len() >= 3);
    }
}
