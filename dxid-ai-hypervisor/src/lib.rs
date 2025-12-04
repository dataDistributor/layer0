use anyhow::Result;
use dxid_config::AiConfig;
use dxid_storage::PgStore;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

pub struct Hypervisor {
    cfg: AiConfig,
    client: Client,
    store: Arc<PgStore>,
}

impl Hypervisor {
    pub fn new(cfg: AiConfig, store: Arc<PgStore>) -> Self {
        Self {
            cfg,
            client: Client::new(),
            store,
        }
    }

    pub async fn query(&self, prompt: &str) -> Result<String> {
        // Build synthetic context
        let summary = json!({
            "height": 0,
            "peers": 0,
            "prompt": prompt,
        });
        let body = json!({
            "model": self.cfg.model,
            "messages": [
                {"role": "system", "content": "You are the dxid AI hypervisor providing concise chain analytics."},
                {"role": "user", "content": format!("Context: {summary}. Question: {prompt}")}
            ]
        });
        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.cfg.openai_api_key)
            .json(&body)
            .send()
            .await?;
        let val: serde_json::Value = resp.json().await?;
        let answer = val["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("No answer")
            .to_string();
        Ok(answer)
    }
}
