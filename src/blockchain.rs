use crate::account::AccountState;
use crate::consensus::ConsensusEngine;
use crate::snapshot::PruningManager;
use crate::storage::Storage;
use crate::{Block, Transaction};
use std::sync::Arc;

pub const MAX_REORG_DEPTH: usize = 100;
pub const FINALITY_DEPTH: usize = 50;

pub struct Blockchain {
    pub chain: Vec<Block>,
    pub consensus: Arc<dyn ConsensusEngine>,
    pub pending_transactions: Vec<Transaction>,
    pub storage: Option<Storage>,
    pub state: AccountState,
    pub chain_id: u64,
    pub pruning_manager: Option<PruningManager>,
}
impl Blockchain {
    pub fn new(
        consensus: Arc<dyn ConsensusEngine>,
        storage: Option<Storage>,
        chain_id: u64,
        pruning_manager: Option<PruningManager>,
    ) -> Self {
        println!("Consensus: {}", consensus.info());
        let mut chain_vec = Vec::new();
        let mut state = AccountState::new();

        let mut loaded_chain = false;
        if let Some(ref store) = storage {
            if let Ok(c) = store.load_chain() {
                if !c.is_empty() {
                    chain_vec = c;
                    loaded_chain = true;
                    println!("📚 Loaded chain from DB: {} blocks", chain_vec.len());
                }
            }
        }

        if !loaded_chain {
            let mut genesis = Block::genesis();
            genesis.chain_id = chain_id;
            genesis.hash = genesis.calculate_hash();
            chain_vec.push(genesis);
        }

        let mut snapshot_height = 0;
        if let Some(ref pm) = pruning_manager {
            if let Ok(Some(snapshot)) = pm.load_latest_snapshot() {
                if snapshot.chain_id == chain_id {
                    for (addr, balance) in &snapshot.balances {
                        let acc = state.get_or_create(addr);
                        acc.balance = *balance;
                    }
                    for (addr, nonce) in &snapshot.nonces {
                        let acc = state.get_or_create(addr);
                        acc.nonce = *nonce;
                    }
                    snapshot_height = snapshot.height;
                    println!(
                        "✅ Restored state from snapshot at height {}",
                        snapshot_height
                    );
                } else {
                    println!(
                        "⚠️  Snapshot chain_id mismatch (expected {}, got {}). Ignoring.",
                        chain_id, snapshot.chain_id
                    );
                }
            }
        }

        let chain_len = chain_vec.len();
        let start_index = if snapshot_height > 0 && snapshot_height < chain_len as u64 {
            (snapshot_height + 1) as usize
        } else {
            if snapshot_height >= chain_len as u64 {
                println!("⚠️  Chain shorter than snapshot height! Replaying from Genesis.");
                0
            } else {
                0
            }
        };

        println!(
            "🔄 Replaying blocks from index {} to {}...",
            start_index,
            chain_len - 1
        );

        for (i, block) in chain_vec.iter().enumerate().skip(start_index) {
            if i == 0 {}

            for tx in &block.transactions {
                if let Err(e) = state.apply_transaction(tx) {
                    println!("❌ Error applying block {} tx: {}", block.index, e);
                }
            }

            let total_fees: u64 = block.transactions.iter().map(|t| t.fee).sum();
            if let Some(ref producer) = block.producer {
                if total_fees > 0 {
                    let acc = state.get_or_create(producer);
                    acc.balance += total_fees;
                }
            }
        }

        Blockchain {
            chain: chain_vec,
            consensus,
            pending_transactions: Vec::new(),
            storage,
            state,
            chain_id,
            pruning_manager,
        }
    }

    fn load_chain_from_db(&mut self, last_hash: String) -> std::io::Result<()> {
        let mut current_hash = last_hash;
        let mut blocks = Vec::new();
        if let Some(ref store) = self.storage {
            while let Ok(Some(block)) = store.get_block(&current_hash) {
                blocks.push(block.clone());
                if block.previous_hash == "0".repeat(64) {
                    break;
                }
                current_hash = block.previous_hash;
            }
        }
        if blocks.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Chain broken or empty",
            ));
        }
        blocks.reverse();
        self.chain = blocks;
        println!("Loaded {} blocks from disk", self.chain.len());
        Ok(())
    }
    fn create_genesis_block(&mut self) {
        let genesis_block = Block::genesis();
        self.chain.push(genesis_block.clone());
        if let Some(ref store) = self.storage {
            let _ = store.insert_block(&genesis_block);
            let _ = store.save_last_hash(&genesis_block.hash);
        }
    }
    pub fn last_block(&self) -> &Block {
        self.chain.last().expect("Chain should never be empty")
    }
    pub fn produce_block(&mut self, producer_address: String) {
        let index = self.chain.len() as u64;
        let previous_hash = self.chain.last().unwrap().hash.clone();

        let mut valid_txs = Vec::new();
        let mut temp_state = self.state.clone();

        for tx in &self.pending_transactions {
            if let Ok(_) = temp_state.validate_transaction(tx) {
                if let Ok(_) = temp_state.apply_transaction(tx) {
                    valid_txs.push(tx.clone());
                }
            } else {
                println!("Discarding invalid transaction: {}", tx.hash);
            }
        }

        let mut block = Block::new(index, previous_hash, valid_txs);
        println!(
            "Producing block {} with {} ({} txs)...",
            index,
            self.consensus.consensus_type(),
            block.transactions.len()
        );

        block.producer = Some(producer_address.clone());

        if let Err(e) = self.consensus.prepare_block(&mut block) {
            println!("Block preparation failed: {}", e);
            return;
        }

        println!("Block produced: {}", block.hash);
        if let Some(ref store) = self.storage {
            let _ = store.insert_block(&block);
            let _ = store.save_last_hash(&block.hash);
        }

        for tx in &block.transactions {
            let _ = self.state.apply_transaction(tx);
        }

        self.chain.push(block);
        self.pending_transactions = Vec::new();
    }
    pub fn mine_pending_transactions(&mut self, miner_address: String) {
        self.produce_block(miner_address);
    }
    pub fn add_transaction(&mut self, transaction: Transaction) -> Result<(), String> {
        if transaction.chain_id != self.chain_id {
            return Err(format!(
                "Invalid Chain ID: expected {}, got {}",
                self.chain_id, transaction.chain_id
            ));
        }
        if let Err(e) = self.state.validate_transaction(&transaction) {
            return Err(format!("Invalid transaction: {}", e));
        }
        self.pending_transactions.push(transaction);
        Ok(())
    }

    pub fn init_genesis_account(&mut self, address: &str) {
        self.state.add_balance(address, 1_000_000_000);
    }

    pub fn validate_and_add_block(&mut self, block: Block) -> Result<(), String> {
        if block.chain_id != self.chain_id {
            return Err(format!(
                "Invalid Chain ID: expected {}, got {}",
                self.chain_id, block.chain_id
            ));
        }

        if let Err(e) = self.consensus.validate_block(&block, &self.chain) {
            return Err(format!("Consensus validation failed: {}", e));
        }

        let mut temp_state = self.state.clone();
        for (i, tx) in block.transactions.iter().enumerate() {
            if let Err(e) = temp_state.apply_transaction(tx) {
                return Err(format!("Invalid transaction at index {}: {}", i, e));
            }
        }

        if let Some(ref store) = self.storage {
            let _ = store.insert_block(&block);
            let _ = store.save_last_hash(&block.hash);
        }

        self.state = temp_state;

        self.chain.push(block);

        let mut new_pending = Vec::new();
        for tx in &self.pending_transactions {
            if self.state.validate_transaction(tx).is_ok() {
                new_pending.push(tx.clone());
            }
        }
        self.pending_transactions = new_pending;

        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        for i in 0..self.chain.len() {
            let block = &self.chain[i];
            let previous_chain = &self.chain[..i];
            if let Err(e) = self.consensus.validate_block(block, previous_chain) {
                println!("Block {} validation failed: {}", i, e);
                return false;
            }
        }
        true
    }
    pub fn is_valid_chain(&self, chain: &[Block]) -> bool {
        if chain.is_empty() {
            return false;
        }
        if chain[0] != Block::genesis() {
            return false;
        }
        for i in 0..chain.len() {
            let block = &chain[i];
            let previous_chain = &chain[..i];
            if let Err(_) = self.consensus.validate_block(block, previous_chain) {
                return false;
            }
        }
        true
    }
    pub fn find_fork_point(&self, other_chain: &[Block]) -> Option<usize> {
        for (i, block) in self.chain.iter().enumerate() {
            if i >= other_chain.len() {
                return None;
            }
            if block.hash != other_chain[i].hash {
                return Some(i);
            }
        }
        None
    }
    pub fn try_reorg(&mut self, new_chain: Vec<Block>) -> Result<bool, String> {
        if new_chain.len() <= self.chain.len() {
            return Ok(false);
        }
        if !self.is_valid_chain(&new_chain) {
            return Err("Invalid chain".to_string());
        }

        let fork_point = self.find_fork_point(&new_chain).unwrap_or(0);
        let reorg_depth = self.chain.len().saturating_sub(fork_point);

        if reorg_depth > MAX_REORG_DEPTH {
            return Err(format!(
                "Reorg depth {} exceeds max {}",
                reorg_depth, MAX_REORG_DEPTH
            ));
        }

        let finalized_height = self.chain.len().saturating_sub(FINALITY_DEPTH);
        if fork_point < finalized_height {
            return Err("Cannot reorg past finality depth".to_string());
        }

        println!(
            "Reorg: replacing {} blocks from height {}",
            reorg_depth, fork_point
        );

        let new_state = Blockchain::rebuild_state(&new_chain)?;

        self.chain = new_chain;
        self.state = new_state;

        let mut new_pending = Vec::new();

        let mut chain_txs = std::collections::HashSet::new();
        for block in &self.chain {
            for tx in &block.transactions {
                chain_txs.insert(tx.hash.clone());
            }
        }

        for tx in &self.pending_transactions {
            if !chain_txs.contains(&tx.hash) {
                if self.state.validate_transaction(tx).is_ok() {
                    new_pending.push(tx.clone());
                }
            }
        }
        self.pending_transactions = new_pending;

        if let Some(ref store) = self.storage {
            if let Some(last) = self.chain.last() {
                let _ = store.save_last_hash(&last.hash);
            }
            for block in &self.chain[fork_point..] {
                let _ = store.insert_block(block);
            }
        }

        Ok(true)
    }

    fn rebuild_state(chain: &[Block]) -> Result<AccountState, String> {
        let mut state = AccountState::new();

        for block in chain.iter() {
            for (tx_idx, tx) in block.transactions.iter().enumerate() {
                if tx.from == "genesis" {
                    continue;
                }
                if let Err(e) = state.apply_transaction(tx) {
                    return Err(format!(
                        "State replay failed at block {} tx {}: {}",
                        block.index, tx_idx, e
                    ));
                }
            }

            let mut total_fees: u64 = 0;
            for tx in &block.transactions {
                if tx.from != "genesis" {
                    total_fees += tx.fee;
                }
            }
            if let Some(producer) = &block.producer {
                if total_fees > 0 {
                    let producer_account = state.get_or_create(producer);
                    producer_account.balance += total_fees;
                }
            }
        }
        Ok(state)
    }
    pub fn print_info(&self) {
        println!("================================");
        println!("Blockchain Info");
        println!("================================");
        println!("Consensus: {}", self.consensus.info());
        println!("Length: {}", self.chain.len());
        println!("Pending Tx: {}", self.pending_transactions.len());
        println!("================================");
        for block in &self.chain {
            println!(" Block #{}: {}", block.index, &block.hash[..16]);
        }
    }
    pub fn consensus(&self) -> &dyn ConsensusEngine {
        self.consensus.as_ref()
    }
}
impl Clone for Blockchain {
    fn clone(&self) -> Self {
        Blockchain {
            chain: self.chain.clone(),
            consensus: Arc::clone(&self.consensus),
            pending_transactions: self.pending_transactions.clone(),
            storage: self.storage.clone(),
            state: self.state.clone(),
            chain_id: self.chain_id,
            pruning_manager: self.pruning_manager.clone(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::PoWEngine;
    use crate::crypto::KeyPair;

    #[test]
    fn test_blockchain_with_pow() {
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);

        let keypair = KeyPair::generate().unwrap();
        let pubkey = keypair.public_key_hex();

        blockchain.state.add_balance(&pubkey, 100);

        let mut tx = Transaction::new(pubkey.clone(), "bob".to_string(), 50, vec![]);
        tx.fee = 1;
        tx.sign(&keypair);

        blockchain.add_transaction(tx).unwrap();

        blockchain.produce_block("miner1".to_string());
        assert!(blockchain.is_valid());
        assert_eq!(blockchain.chain.len(), 2);
    }
}
