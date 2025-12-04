use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::RwLock;

#[async_trait]
pub trait Contract: Send + Sync {
    fn id(&self) -> &str;
    async fn execute(&self, input: Value) -> Result<Value>;
}

pub struct ContractRegistry {
    contracts: RwLock<HashMap<String, Box<dyn Contract>>>,
}

impl ContractRegistry {
    pub fn new() -> Self {
        Self {
            contracts: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(&self, contract: Box<dyn Contract>) {
        let mut map = self.contracts.write().await;
        map.insert(contract.id().to_string(), contract);
    }

    pub async fn call(&self, id: &str, input: Value) -> Result<Value> {
        let map = self.contracts.read().await;
        let contract = map.get(id).ok_or_else(|| anyhow::anyhow!("contract not found"))?;
        contract.execute(input).await
    }
}

pub struct KvContract {
    store: RwLock<HashMap<String, String>>,
}

impl KvContract {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Contract for KvContract {
    fn id(&self) -> &str {
        "kv"
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let op = input
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing op"))?;
        match op {
            "set" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing key"))?;
                let value = input
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing value"))?;
                self.store.write().await.insert(key.into(), value.into());
                Ok(serde_json::json!({"status": "ok"}))
            }
            "get" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing key"))?;
                let val = self.store.read().await.get(key).cloned();
                Ok(serde_json::json!({ "value": val }))
            }
            _ => Err(anyhow::anyhow!("unsupported op")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn kv_contract_flow() {
        let kv = KvContract::new();
        let registry = ContractRegistry::new();
        registry.register(Box::new(kv)).await;
        registry
            .call("kv", serde_json::json!({"op":"set","key":"foo","value":"bar"}))
            .await
            .unwrap();
        let res = registry
            .call("kv", serde_json::json!({"op":"get","key":"foo"}))
            .await
            .unwrap();
        assert_eq!(res.get("value").unwrap().as_str().unwrap(), "bar");
    }
}
