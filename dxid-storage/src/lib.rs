use anyhow::Result;
use async_trait::async_trait;
use dxid_core::{Address, Block, Identity, IdentityId};
use dxid_vectors::{Embedding, EmbeddingId};
use pgvector::Vector;
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use tracing::info;

#[async_trait]
pub trait BlockStore: Send + Sync {
    async fn insert_block(&self, block: &Block) -> Result<()>;
    async fn get_block_by_height(&self, height: i64) -> Result<Option<Block>>;
}

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn get_balance(&self, addr: &Address) -> Result<u64>;
    async fn set_balance(&self, addr: &Address, value: u64) -> Result<()>;
}

#[async_trait]
pub trait IdentityStore: Send + Sync {
    async fn put_identity(&self, identity: &Identity) -> Result<()>;
    async fn get_identity(&self, id: &IdentityId) -> Result<Option<Identity>>;
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn insert_embedding(&self, embedding: &Embedding) -> Result<()>;
    async fn knn_search(&self, space: &str, query: &[f32], k: i64) -> Result<Vec<Embedding>>;
}

#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> Result<()> {
        // Minimal schema creation. In production this would be handled by migration files.
        sqlx::query(
            r#"
        CREATE TABLE IF NOT EXISTS blocks(
            height BIGINT PRIMARY KEY,
            data JSONB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS balances(
            address BYTEA PRIMARY KEY,
            amount BIGINT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS identities(
            id UUID PRIMARY KEY,
            data JSONB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS embeddings(
            id TEXT PRIMARY KEY,
            namespace TEXT NOT NULL,
            vector VECTOR(1536) NOT NULL,
            metadata JSONB NOT NULL
        );
        "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl BlockStore for PgStore {
    async fn insert_block(&self, block: &Block) -> Result<()> {
        sqlx::query("INSERT INTO blocks(height, data) VALUES ($1, $2) ON CONFLICT (height) DO UPDATE SET data = EXCLUDED.data")
            .bind(block.header.height as i64)
            .bind(json!(block))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_block_by_height(&self, height: i64) -> Result<Option<Block>> {
        let row = sqlx::query("SELECT data FROM blocks WHERE height = $1")
            .bind(height)
            .fetch_optional(&self.pool)
            .await?;
        if let Some(row) = row {
            let value: serde_json::Value = row.try_get("data")?;
            let blk: Block = serde_json::from_value(value)?;
            return Ok(Some(blk));
        }
        Ok(None)
    }
}

#[async_trait]
impl StateStore for PgStore {
    async fn get_balance(&self, addr: &Address) -> Result<u64> {
        let row = sqlx::query("SELECT amount FROM balances WHERE address = $1")
            .bind(addr.as_slice())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row
            .map(|r| {
                let v: i64 = r.try_get("amount").unwrap_or(0);
                v as u64
            })
            .unwrap_or(0))
    }

    async fn set_balance(&self, addr: &Address, value: u64) -> Result<()> {
        sqlx::query(
            "INSERT INTO balances(address, amount) VALUES ($1, $2) ON CONFLICT (address) DO UPDATE SET amount = EXCLUDED.amount",
        )
        .bind(addr.as_slice())
        .bind(value as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl IdentityStore for PgStore {
    async fn put_identity(&self, identity: &Identity) -> Result<()> {
        sqlx::query(
            "INSERT INTO identities(id, data) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET data = EXCLUDED.data",
        )
        .bind(identity.id)
        .bind(json!(identity))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_identity(&self, id: &IdentityId) -> Result<Option<Identity>> {
        let row = sqlx::query("SELECT data FROM identities WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        if let Some(row) = row {
            let value: serde_json::Value = row.try_get("data")?;
            let identity: Identity = serde_json::from_value(value)?;
            Ok(Some(identity))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl VectorStore for PgStore {
    async fn insert_embedding(&self, embedding: &Embedding) -> Result<()> {
        sqlx::query(
            "INSERT INTO embeddings(id, namespace, vector, metadata) VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET vector = EXCLUDED.vector, metadata = EXCLUDED.metadata",
        )
        .bind(&embedding.id.0)
        .bind(&embedding.namespace)
        .bind(Vector::from(embedding.values.clone()))
        .bind(json!(embedding.metadata))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn knn_search(&self, space: &str, query: &[f32], k: i64) -> Result<Vec<Embedding>> {
        let rows = sqlx::query(
            "SELECT id, namespace, metadata, vector <-> $1 as dist FROM embeddings
             WHERE namespace = $2 ORDER BY vector <-> $1 LIMIT $3",
        )
        .bind(Vector::from(query.to_vec()))
        .bind(space)
        .bind(k)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for row in rows {
            let id: String = row.try_get("id")?;
            let namespace: String = row.try_get("namespace")?;
            let metadata: serde_json::Value = row.try_get("metadata").unwrap_or_default();
            out.push(Embedding {
                id: EmbeddingId(id),
                namespace,
                values: query.to_vec(), // keep payload lean
                metadata,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dxid_core::{TxInput, TxOutput};
    use sqlx::Executor;

    // Uses an in-memory postgres replacement is not available, so mark the test ignored.
    #[tokio::test]
    #[ignore]
    async fn embed_and_query() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL for test");
        let store = PgStore::connect(&url, 5).await.unwrap();
        let emb = Embedding::new("space".into(), vec![0.1, 0.2, 0.3], json!({"label": "demo"}));
        store.insert_embedding(&emb).await.unwrap();
        let res = store.knn_search("space", &[0.1, 0.2, 0.4], 5).await.unwrap();
        assert!(!res.is_empty());
    }
}
