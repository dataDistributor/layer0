use anyhow::{anyhow, Result};
use async_trait::async_trait;
use blake3::Hasher;
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Address is derived from a public key hash and is 32 bytes.
pub type Address = [u8; 32];
pub type TxHash = [u8; 32];
pub type BlockHash = [u8; 32];
pub type ChainId = String;
pub type IdentityId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum IdentityStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityAttribute {
    pub key: String,
    pub value: String,
    /// Optional reference to an embedding stored in pgvector.
    pub embedding_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub id: IdentityId,
    pub public_keys: Vec<Vec<u8>>,
    pub attributes: HashMap<String, IdentityAttribute>,
    pub status: IdentityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxInput {
    pub previous_tx: TxHash,
    pub output_index: u32,
    pub signature: Vec<u8>,
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxOutput {
    pub address: Address,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub fee: u64,
    pub nonce: u64,
    pub memo: Option<String>,
}

impl Transaction {
    pub fn hash(&self) -> TxHash {
        let mut hasher = Hasher::new();
        let encoded = serde_json::to_vec(self).unwrap();
        hasher.update(&encoded);
        hasher.finalize().into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub previous_hash: BlockHash,
    pub merkle_root: BlockHash,
    pub height: u64,
    pub timestamp: u64,
    pub difficulty: u64,
    pub nonce: u64,
    pub validator: Address,
    pub stake_weight: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
    pub pow_hash: BlockHash,
    pub validator_signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainMetadata {
    pub chain_id: ChainId,
    pub rpc_endpoint: String,
    pub latest_height: u64,
    pub network: String,
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainMessage {
    pub id: Uuid,
    pub source: ChainId,
    pub dest: ChainId,
    pub payload: serde_json::Value,
    pub nonce: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainTx {
    pub message: CrossChainMessage,
    pub fee: u64,
    pub proof: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HalvingSchedule {
    /// Blocks between halvings if height-based.
    pub target_interval: u64,
    /// Supply threshold that forces halving.
    pub supply_threshold: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenEconomics {
    pub max_supply: u64,
    pub base_reward: u64,
    pub schedule: HalvingSchedule,
    pub treasury_ratio_bps: u16,
}

#[derive(Debug, Clone, Default)]
pub struct ChainState {
    pub balances: HashMap<Address, u64>,
    pub identities: HashMap<IdentityId, Identity>,
    pub chain_links: HashMap<ChainId, ChainMetadata>,
    pub total_issued: u64,
    pub issued_rewards: u64,
    pub pending_utxos: HashMap<TxHash, Vec<TxOutput>>,
}

#[async_trait]
pub trait CryptoProvider: Send + Sync + 'static {
    fn address_from_public_key(&self, pk: &[u8]) -> Result<Address>;
    fn verify_signature(&self, pk: &[u8], msg: &[u8], sig: &[u8]) -> Result<bool>;
    fn sign_message(&self, sk: &[u8], msg: &[u8]) -> Result<Vec<u8>>;
    fn hash_block_header(&self, header: &BlockHeader) -> BlockHash;
}

pub struct ExecutionEngine<'a, C: CryptoProvider> {
    pub crypto: &'a C,
    pub economics: TokenEconomics,
}

impl<'a, C: CryptoProvider> ExecutionEngine<'a, C> {
    pub fn new(crypto: &'a C, economics: TokenEconomics) -> Self {
        Self { crypto, economics }
    }

    pub fn current_reward(&self, height: u64, total_issued: u64) -> u64 {
        let halvings_by_height = if self.economics.schedule.target_interval == 0 {
            0
        } else {
            height / self.economics.schedule.target_interval
        };
        let halvings_by_supply = if self.economics.schedule.supply_threshold == 0 {
            0
        } else {
            total_issued / self.economics.schedule.supply_threshold
        };
        let halvings = halvings_by_height.max(halvings_by_supply);
        self.economics
            .base_reward
            .checked_shr(halvings as u32)
            .unwrap_or(0)
    }

    pub fn apply_block(&self, state: &mut ChainState, block: &Block) -> Result<()> {
        // Verify block hash target (PoW) and validator signature are performed upstream.
        let merkle = merkle_root(&block.transactions);
        if merkle != block.header.merkle_root {
            return Err(anyhow!("invalid merkle root"));
        }
        // ensure monotonic height
        if block.header.height != 0 && block.header.height != self.next_height(state)? {
            return Err(anyhow!("unexpected height"));
        }
        let mut spent: HashSet<(TxHash, u32)> = HashSet::new();
        for tx in &block.transactions {
            self.apply_transaction(state, tx, &mut spent)?;
        }
        let reward = self.current_reward(block.header.height, state.total_issued);
        let treasury_cut = reward * self.economics.treasury_ratio_bps as u64 / 10_000;
        let miner_reward = reward.saturating_sub(treasury_cut);
        Self::credit(state, &block.header.validator, miner_reward)?;
        state.total_issued = (state.total_issued + reward).min(self.economics.max_supply);
        state.issued_rewards += reward;
        Ok(())
    }

    fn apply_transaction(
        &self,
        state: &mut ChainState,
        tx: &Transaction,
        spent: &mut HashSet<(TxHash, u32)>,
    ) -> Result<()> {
        let tx_hash = tx.hash();
        let mut input_total = 0u64;
        if tx.inputs.is_empty() && tx.outputs.is_empty() {
            return Err(anyhow!("empty transaction"));
        }
        for input in &tx.inputs {
            if !spent.insert((input.previous_tx, input.output_index)) {
                return Err(anyhow!("double spend detected"));
            }
            let prev_outputs = state
                .pending_utxos
                .get(&input.previous_tx)
                .ok_or_else(|| anyhow!("missing previous tx"))?;
            let output = prev_outputs
                .get(input.output_index as usize)
                .ok_or_else(|| anyhow!("missing output index"))?;
            let pk_hash = self.crypto.address_from_public_key(&input.public_key)?;
            if pk_hash != output.address {
                return Err(anyhow!("input not owned by signer"));
            }
            let mut msg = Vec::new();
            msg.extend_from_slice(&input.previous_tx);
            msg.extend_from_slice(&input.output_index.to_le_bytes());
            msg.extend_from_slice(&tx_hash);
            if !self.crypto.verify_signature(&input.public_key, &msg, &input.signature)? {
                return Err(anyhow!("signature invalid"));
            }
            input_total = input_total
                .checked_add(output.amount)
                .ok_or_else(|| anyhow!("input overflow"))?;
        }
        let mut output_total = 0u64;
        for out in &tx.outputs {
            output_total = output_total
                .checked_add(out.amount)
                .ok_or_else(|| anyhow!("output overflow"))?;
        }
        if input_total < output_total + tx.fee {
            return Err(anyhow!("insufficient input amount"));
        }
        // Update balances and UTXO set
        for out in &tx.outputs {
            Self::credit(state, &out.address, out.amount)?;
        }
        // Remove spent outputs
        for input in &tx.inputs {
            if let Some(prev_outputs) = state.pending_utxos.get_mut(&input.previous_tx) {
                if input.output_index as usize >= prev_outputs.len() {
                    return Err(anyhow!("output index out of bounds"));
                }
                prev_outputs[input.output_index as usize].amount = 0;
            }
        }
        state
            .pending_utxos
            .insert(tx_hash, tx.outputs.clone());
        Ok(())
    }

    fn credit(state: &mut ChainState, addr: &Address, amount: u64) -> Result<()> {
        let entry = state.balances.entry(*addr).or_insert(0);
        *entry = entry
            .checked_add(amount)
            .ok_or_else(|| anyhow!("balance overflow"))?;
        Ok(())
    }

    fn next_height(&self, state: &ChainState) -> Result<u64> {
        // Approximate next height by counting blocks from issued rewards + transactions.
        // In a real system height tracking would be stored; here we derive from UTXO length.
        let computed = state
            .pending_utxos
            .len()
            .try_into()
            .unwrap_or_default();
        Ok(computed)
    }
}

pub fn merkle_root(transactions: &[Transaction]) -> BlockHash {
    if transactions.is_empty() {
        return [0u8; 32];
    }
    let mut hashes: Vec<BlockHash> = transactions.iter().map(|tx| tx.hash()).collect();
    while hashes.len() > 1 {
        let mut next = Vec::new();
        for pair in hashes.chunks(2) {
            let mut hasher = Hasher::new();
            hasher.update(&pair[0]);
            if pair.len() == 2 {
                hasher.update(&pair[1]);
            } else {
                hasher.update(&pair[0]);
            }
            next.push(hasher.finalize().into());
        }
        hashes = next;
    }
    hashes[0]
}

pub fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn random_nonce() -> u64 {
    rand::thread_rng().next_u64()
}

pub fn new_identity(initial_pk: Vec<u8>) -> Identity {
    Identity {
        id: Uuid::new_v4(),
        public_keys: vec![initial_pk],
        attributes: HashMap::new(),
        status: IdentityStatus::Active,
    }
}

pub fn add_attribute(identity: &mut Identity, attr: IdentityAttribute) {
    identity.attributes.insert(attr.key.clone(), attr);
}

pub fn rotate_identity_key(identity: &mut Identity, new_pk: Vec<u8>) {
    identity.public_keys.push(new_pk);
}

pub fn revoke_identity(identity: &mut Identity) {
    identity.status = IdentityStatus::Revoked;
}

pub fn authorize_identity_proof(
    identity: &Identity,
    attribute_predicate: Option<(&str, &dyn Fn(&IdentityAttribute) -> bool)>,
) -> bool {
    if identity.status != IdentityStatus::Active {
        return false;
    }
    if let Some((key, predicate)) = attribute_predicate {
        if let Some(attr) = identity.attributes.get(key) {
            return predicate(attr);
        }
        return false;
    }
    true
}

pub fn chain_metadata(chain_id: ChainId, rpc_endpoint: String) -> ChainMetadata {
    ChainMetadata {
        chain_id,
        rpc_endpoint,
        latest_height: 0,
        network: "main".to_string(),
        extra: serde_json::json!({}),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthLikeProofRequest {
    pub audience: String,
    pub scope: Vec<String>,
    pub nonce: Uuid,
    pub challenge: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthLikeProofResponse {
    pub identity_id: IdentityId,
    pub issued_at: DateTime<Utc>,
    pub signature: Vec<u8>,
    pub disclosed_attributes: HashMap<String, String>,
}

pub fn build_oauth_like_challenge(audience: String, scope: Vec<String>) -> OAuthLikeProofRequest {
    let mut rng = rand::thread_rng();
    let mut challenge = vec![0u8; 32];
    rng.fill_bytes(&mut challenge);
    OAuthLikeProofRequest {
        audience,
        scope,
        nonce: Uuid::new_v4(),
        challenge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyCrypto;

    #[async_trait]
    impl CryptoProvider for DummyCrypto {
        fn address_from_public_key(&self, pk: &[u8]) -> Result<Address> {
            let mut hash = blake3::hash(pk).as_bytes().clone();
            Ok(hash)
        }

        fn verify_signature(&self, _pk: &[u8], _msg: &[u8], _sig: &[u8]) -> Result<bool> {
            Ok(true)
        }

        fn sign_message(&self, _sk: &[u8], msg: &[u8]) -> Result<Vec<u8>> {
            Ok(msg.to_vec())
        }

        fn hash_block_header(&self, header: &BlockHeader) -> BlockHash {
            let bytes = serde_json::to_vec(header).unwrap();
            blake3::hash(&bytes).into()
        }
    }

    #[test]
    fn merkle_single() {
        let tx = Transaction {
            inputs: vec![],
            outputs: vec![TxOutput {
                address: [1u8; 32],
                amount: 10,
            }],
            fee: 0,
            nonce: 0,
            memo: None,
        };
        let root = merkle_root(&[tx.clone()]);
        assert_eq!(root, tx.hash());
    }

    #[test]
    fn apply_block_flow() {
        let crypto = DummyCrypto;
        let economics = TokenEconomics {
            max_supply: 21_000_000_0000,
            base_reward: 50_0000,
            schedule: HalvingSchedule {
                target_interval: 10,
                supply_threshold: 1_000_000_000,
            },
            treasury_ratio_bps: 500,
        };
        let engine = ExecutionEngine::new(&crypto, economics);
        let mut state = ChainState::default();
        let tx = Transaction {
            inputs: vec![],
            outputs: vec![TxOutput {
                address: [2u8; 32],
                amount: 10,
            }],
            fee: 0,
            nonce: 1,
            memo: Some("genesis".into()),
        };
        let block = Block {
            header: BlockHeader {
                previous_hash: [0u8; 32],
                merkle_root: merkle_root(&[tx.clone()]),
                height: 0,
                timestamp: now_ts(),
                difficulty: 1,
                nonce: 0,
                validator: [9u8; 32],
                stake_weight: 1,
            },
            transactions: vec![tx],
            pow_hash: [0u8; 32],
            validator_signature: vec![],
        };
        engine.apply_block(&mut state, &block).unwrap();
        assert!(state.total_issued > 0);
        assert_eq!(state.balances.get(&[2u8; 32]).cloned().unwrap_or(0), 10);
    }
}
