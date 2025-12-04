use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dxid_core::{merkle_root, now_ts, Address, Block, BlockHeader, CryptoProvider, Transaction};
use dxid_crypto::DefaultCryptoProvider;
use parking_lot::RwLock;
use rand::{seq::IteratorRandom, Rng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub pow_target_spacing: u64,
    pub difficulty_window: usize,
    pub max_supply: u64,
    pub base_reward: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsensusState {
    pub difficulty: u64,
    pub stakes: HashMap<Address, u64>,
    pub last_height: u64,
}

#[async_trait]
pub trait ConsensusEngine: Send + Sync {
    fn propose_block(
        &self,
        previous: &BlockHeader,
        transactions: Vec<Transaction>,
        validator: Address,
    ) -> Result<Block>;
    fn validate_block(&self, block: &Block) -> Result<()>;
    fn stake(&self, addr: Address, amount: u64) -> Result<()>;
    fn unstake(&self, addr: &Address, amount: u64) -> Result<()>;
    fn slashing(&self, addr: &Address, amount: u64) -> Result<()>;
    fn state(&self) -> ConsensusState;
}

pub struct HybridConsensus<C: CryptoProvider> {
    crypto: Arc<C>,
    state: RwLock<ConsensusState>,
    config: ConsensusConfig,
}

impl<C: CryptoProvider> HybridConsensus<C> {
    pub fn new(crypto: Arc<C>, config: ConsensusConfig) -> Self {
        Self {
            crypto,
            state: RwLock::new(ConsensusState {
                difficulty: 0x00ff_ffff,
                stakes: HashMap::new(),
                last_height: 0,
            }),
            config,
        }
    }

    fn target_from_difficulty(&self, difficulty: u64) -> u128 {
        // lower target = harder. We invert difficulty for demonstration.
        let base: u128 = u128::MAX / (difficulty as u128 + 1);
        base
    }

    fn pow_hash(&self, header: &BlockHeader) -> u128 {
        let hash: [u8; 32] = self.crypto.hash_block_header(header);
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&hash[0..16]);
        u128::from_le_bytes(bytes)
    }

    fn select_validator(&self) -> Option<Address> {
        let state = self.state.read();
        let total_stake: u128 = state.stakes.values().map(|v| *v as u128).sum();
        if total_stake == 0 {
            return None;
        }
        let mut rng = rand::thread_rng();
        let mut pick = rng.gen_range(0..total_stake);
        for (addr, stake) in state.stakes.iter() {
            if pick < *stake as u128 {
                return Some(*addr);
            }
            pick -= *stake as u128;
        }
        None
    }
}

#[async_trait]
impl<C: CryptoProvider> ConsensusEngine for HybridConsensus<C> {
    fn propose_block(
        &self,
        previous: &BlockHeader,
        transactions: Vec<Transaction>,
        validator: Address,
    ) -> Result<Block> {
        let mut header = BlockHeader {
            previous_hash: self.crypto.hash_block_header(previous),
            merkle_root: merkle_root(&transactions),
            height: previous.height + 1,
            timestamp: now_ts(),
            difficulty: self.state.read().difficulty,
            nonce: 0,
            validator,
            stake_weight: *self.state.read().stakes.get(&validator).unwrap_or(&0),
        };
        let target = self.target_from_difficulty(header.difficulty);
        let mut rng = rand::thread_rng();
        let mut pow_hash_val;
        loop {
            header.nonce = rng.gen();
            pow_hash_val = self.pow_hash(&header);
            if pow_hash_val < target {
                break;
            }
        }
        let pow_bytes: BlockHash = self.crypto.hash_block_header(&header);
        Ok(Block {
            header,
            transactions,
            pow_hash: pow_bytes,
            validator_signature: vec![],
        })
    }

    fn validate_block(&self, block: &Block) -> Result<()> {
        let state = self.state.read();
        if block.header.height != state.last_height + 1 {
            return Err(anyhow!("unexpected height"));
        }
        let target = self.target_from_difficulty(block.header.difficulty);
        let pow_val = self.pow_hash(&block.header);
        if pow_val >= target {
            return Err(anyhow!("pow target not met"));
        }
        // Check validator stake
        if *state.stakes.get(&block.header.validator).unwrap_or(&0) == 0 {
            return Err(anyhow!("validator not staked"));
        }
        // Basic merkle check
        if block.header.merkle_root != merkle_root(&block.transactions) {
            return Err(anyhow!("merkle mismatch"));
        }
        Ok(())
    }

    fn stake(&self, addr: Address, amount: u64) -> Result<()> {
        let mut state = self.state.write();
        let entry = state.stakes.entry(addr).or_insert(0);
        *entry = entry.saturating_add(amount);
        Ok(())
    }

    fn unstake(&self, addr: &Address, amount: u64) -> Result<()> {
        let mut state = self.state.write();
        let entry = state.stakes.entry(*addr).or_insert(0);
        if *entry < amount {
            return Err(anyhow!("insufficient stake"));
        }
        *entry -= amount;
        Ok(())
    }

    fn slashing(&self, addr: &Address, amount: u64) -> Result<()> {
        let mut state = self.state.write();
        if let Some(stake) = state.stakes.get_mut(addr) {
            *stake = stake.saturating_sub(amount);
        }
        Ok(())
    }

    fn state(&self) -> ConsensusState {
        self.state.read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dxid_core::{TxInput, TxOutput};
    use dxid_crypto::generate_ed25519;

    #[test]
    fn pow_and_pos_flow() {
        let crypto = Arc::new(DefaultCryptoProvider::new());
        let config = ConsensusConfig {
            pow_target_spacing: 30,
            difficulty_window: 10,
            max_supply: 21_000_000_0000,
            base_reward: 50_0000,
        };
        let engine = HybridConsensus::new(crypto.clone(), config);
        let key = generate_ed25519();
        let addr = crypto.address_from_public_key(&key.public_key).unwrap();
        engine.stake(addr, 100).unwrap();

        let tx = Transaction {
            inputs: vec![],
            outputs: vec![TxOutput {
                address: addr,
                amount: 10,
            }],
            fee: 0,
            nonce: 0,
            memo: None,
        };
        let genesis_header = BlockHeader {
            previous_hash: [0u8; 32],
            merkle_root: merkle_root(&[tx.clone()]),
            height: 0,
            timestamp: now_ts(),
            difficulty: 1,
            nonce: 0,
            validator: addr,
            stake_weight: 0,
        };
        let block = engine
            .propose_block(&genesis_header, vec![tx], addr)
            .unwrap();
        engine.validate_block(&block).unwrap();
    }
}
