use crate::consensus::ConsensusEngine;
use crate::storage::Storage;
use crate::{Block, Transaction};
use std::sync::Arc;
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub consensus: Arc<dyn ConsensusEngine>,
    pub pending_transactions: Vec<Transaction>,
    pub storage: Option<Storage>,
}
impl Blockchain {
    pub fn new(consensus: Arc<dyn ConsensusEngine>, storage: Option<Storage>) -> Self {
        println!("Consensus: {}", consensus.info());
        let mut chain = Blockchain {
            chain: Vec::new(),
            consensus,
            pending_transactions: Vec::new(),
            storage,
        };
        if let Some(ref store) = chain.storage {
            if let Ok(Some(last_hash)) = store.get_last_hash() {
                println!("Found existing chain tip: {}", last_hash);
                if let Err(e) = chain.load_chain_from_db(last_hash) {
                    println!("Failed to load chain: {}", e);
                    chain.chain.clear();
                    chain.create_genesis_block();
                }
            } else {
                chain.create_genesis_block();
            }
        } else {
            chain.create_genesis_block();
        }
        chain
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
    pub fn produce_block(&mut self, _producer_address: String) {
        let index = self.chain.len() as u64;
        let previous_hash = self.chain.last().unwrap().hash.clone();
        let mut block = Block::new(index, previous_hash, self.pending_transactions.clone());
        println!(
            "Producing block {} with {}...",
            index,
            self.consensus.consensus_type()
        );
        if let Err(e) = self.consensus.prepare_block(&mut block) {
            println!("Block preparation failed: {}", e);
            return;
        }
        println!("Block produced: {}", block.hash);
        if let Some(ref store) = self.storage {
            let _ = store.insert_block(&block);
            let _ = store.save_last_hash(&block.hash);
        }
        self.chain.push(block);
        self.pending_transactions = Vec::new();
    }
    pub fn mine_pending_transactions(&mut self, miner_address: String) {
        self.produce_block(miner_address);
    }
    pub fn add_transaction(&mut self, transaction: Transaction) {
        self.pending_transactions.push(transaction);
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
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::PoWEngine;
    #[test]
    fn test_blockchain_with_pow() {
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None);
        blockchain.add_transaction(Transaction::new(
            "alice".to_string(),
            "bob".to_string(),
            50,
            vec![],
        ));
        blockchain.produce_block("miner1".to_string());
        assert!(blockchain.is_valid());
        assert_eq!(blockchain.chain.len(), 2);
    }
}
