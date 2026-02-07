use super::{ConsensusEngine, ConsensusError};
use crate::Block;
use sha3::{Digest, Sha3_256};
use std::collections::HashMap;
#[derive(Debug, Clone)]
pub struct PoAConfig {
    pub block_period: u64,
    pub epoch_length: u64,
    pub quorum_ratio: f64,
    pub validators_file: Option<String>,
}
impl Default for PoAConfig {
    fn default() -> Self {
        PoAConfig {
            block_period: 5,
            epoch_length: 30000,
            quorum_ratio: 0.67,
            validators_file: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Authority {
    pub address: String,
    pub active: bool,
    pub last_block_time: u128,
    pub votes_for: usize,
    pub votes_against: usize,
}
impl Authority {
    pub fn new(address: String) -> Self {
        Authority {
            address,
            active: true,
            last_block_time: 0,
            votes_for: 0,
            votes_against: 0,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum VoteType {
    Add,
    Remove,
}
pub struct PoAEngine {
    pub config: PoAConfig,
    pub authorities: Vec<Authority>,
    pub pending_votes: HashMap<String, Vec<(String, VoteType)>>,
    last_producer: Option<String>,
}
impl PoAEngine {
    pub fn new(validators: Vec<String>) -> Self {
        let authorities = validators
            .into_iter()
            .map(|addr| Authority::new(addr))
            .collect();
        PoAEngine {
            config: PoAConfig::default(),
            authorities,
            pending_votes: HashMap::new(),
            last_producer: None,
        }
    }
    pub fn with_config(config: PoAConfig, validators: Vec<String>) -> Self {
        let mut engine = Self::new(validators);
        engine.config = config;
        engine
    }
    pub fn add_authority(&mut self, address: String) {
        if !self.authorities.iter().any(|a| a.address == address) {
            println!("👥 Authority added: {}", address);
            self.authorities.push(Authority::new(address));
        }
    }
    pub fn remove_authority(&mut self, address: &str) {
        if let Some(pos) = self.authorities.iter().position(|a| a.address == address) {
            println!("👥 Authority removed: {}", address);
            self.authorities.remove(pos);
        }
    }
    pub fn expected_proposer(&self, block_index: u64) -> Option<&Authority> {
        let active: Vec<_> = self.authorities.iter().filter(|a| a.active).collect();
        if active.is_empty() {
            return None;
        }
        let slot = (block_index as usize) % active.len();
        Some(active[slot])
    }
    pub fn is_valid_proposer(&self, address: &str, block_index: u64) -> bool {
        self.expected_proposer(block_index)
            .map_or(false, |p| p.address == address)
    }
    pub fn vote(
        &mut self,
        voter: &str,
        target: String,
        vote_type: VoteType,
    ) -> Result<bool, ConsensusError> {
        if !self
            .authorities
            .iter()
            .any(|a| a.address == voter && a.active)
        {
            return Err(ConsensusError("Only validators can vote".into()));
        }
        let votes = self
            .pending_votes
            .entry(target.clone())
            .or_insert_with(Vec::new);
        if votes.iter().any(|(v, _)| v == voter) {
            return Err(ConsensusError("Already voted".into()));
        }
        votes.push((voter.to_string(), vote_type.clone()));
        let required = (self.authorities.len() as f64 * self.config.quorum_ratio).ceil() as usize;
        let vote_count = votes.iter().filter(|(_, t)| t == &vote_type).count();
        if vote_count >= required {
            match vote_type {
                VoteType::Add => {
                    self.add_authority(target.clone());
                    println!("✅ Vote passed: {} added as validator", target);
                }
                VoteType::Remove => {
                    self.remove_authority(&target);
                    println!("✅ Vote passed: {} removed from validators", target);
                }
            }
            self.pending_votes.remove(&target);
            return Ok(true);
        }
        Ok(false)
    }
    fn create_signature(&self, signer: &str, block: &Block) -> Vec<u8> {
        let mut hasher = Sha3_256::new();
        hasher.update(signer.as_bytes());
        hasher.update(block.hash.as_bytes());
        hasher.update(block.index.to_le_bytes());
        hasher.finalize().to_vec()
    }
    fn verify_signature(&self, _signer: &str, _block: &Block, _signature: &[u8]) -> bool {
        true
    }
    pub fn active_validator_count(&self) -> usize {
        self.authorities.iter().filter(|a| a.active).count()
    }
    #[allow(dead_code)]
    fn ibft_pre_prepare(&self, block: &Block) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&[0x00]);
        msg.extend_from_slice(&block.index.to_le_bytes());
        msg.extend_from_slice(block.hash.as_bytes());
        msg
    }
}
impl ConsensusEngine for PoAEngine {
    fn prepare_block(&self, block: &mut Block) -> Result<(), ConsensusError> {
        let slot = block.index;
        let expected_signer = if let Some(expected) = self.expected_proposer(slot) {
            println!(
                "👥 PoA: Block {} should be proposed by: {}",
                slot, expected.address
            );
            Some(expected.address.clone())
        } else if self.authorities.is_empty() {
            println!("⚠️  PoA: No validators configured, allowing block production");
            None
        } else {
            return Err(ConsensusError("No valid proposer for this slot".into()));
        };
        if let Some(signer) = expected_signer {
            block.sign_with_producer(&signer);
            println!("✍️  PoA: Block {} signed by {}", block.index, signer);
        } else {
            block.hash = block.calculate_hash();
        }
        println!("👥 PoA: Block {} prepared", block.index);
        Ok(())
    }
    fn validate_block(&self, block: &Block, chain: &[Block]) -> Result<(), ConsensusError> {
        if block.index == 0 {
            if block.hash != block.calculate_hash() {
                return Err(ConsensusError("Invalid genesis block hash".into()));
            }
            return Ok(());
        }
        if let Some(prev_block) = chain.last() {
            if block.previous_hash != prev_block.hash {
                return Err(ConsensusError(format!(
                    "Previous hash mismatch. Expected: {}, Got: {}",
                    prev_block.hash, block.previous_hash
                )));
            }
        }
        if !self.authorities.is_empty() {
            let expected = self
                .expected_proposer(block.index)
                .ok_or_else(|| ConsensusError("No proposer for this slot".into()))?;
            let producer = block
                .producer
                .as_ref()
                .ok_or_else(|| ConsensusError("Block has no producer".into()))?;
            if producer != &expected.address {
                return Err(ConsensusError(format!(
                    "Wrong proposer. Expected: {}, Got: {}",
                    expected.address, producer
                )));
            }
            if !block.verify_producer_signature(&expected.address) {
                return Err(ConsensusError("Invalid block signature".into()));
            }
            println!(
                "✅ PoA: Block {} signature verified (producer: {})",
                block.index, producer
            );
        } else {
            if block.hash != block.calculate_hash() {
                return Err(ConsensusError("Invalid block hash".into()));
            }
        }
        Ok(())
    }
    fn consensus_type(&self) -> &'static str {
        "PoA"
    }
    fn info(&self) -> String {
        let active_count = self.active_validator_count();
        format!(
            "PoA (validators: {}/{}, quorum: {:.0}%)",
            active_count,
            self.authorities.len(),
            self.config.quorum_ratio * 100.0
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_round_robin() {
        let engine = PoAEngine::new(vec!["alice".into(), "bob".into(), "charlie".into()]);
        assert_eq!(engine.expected_proposer(0).unwrap().address, "alice");
        assert_eq!(engine.expected_proposer(1).unwrap().address, "bob");
        assert_eq!(engine.expected_proposer(2).unwrap().address, "charlie");
        assert_eq!(engine.expected_proposer(3).unwrap().address, "alice");
    }
    #[test]
    fn test_add_remove_authority() {
        let mut engine = PoAEngine::new(vec!["alice".into(), "bob".into()]);
        assert_eq!(engine.active_validator_count(), 2);
        engine.add_authority("charlie".into());
        assert_eq!(engine.active_validator_count(), 3);
        engine.remove_authority("bob");
        assert_eq!(engine.active_validator_count(), 2);
    }
    #[test]
    fn test_voting() {
        let mut engine = PoAEngine::new(vec![
            "alice".into(),
            "bob".into(),
            "charlie".into(),
            "eve".into(),
        ]);
        let result = engine.vote("alice", "dave".into(), VoteType::Add).unwrap();
        assert!(!result);
        let result = engine.vote("bob", "dave".into(), VoteType::Add).unwrap();
        assert!(!result);
        let result = engine
            .vote("charlie", "dave".into(), VoteType::Add)
            .unwrap();
        assert!(result);
        assert_eq!(engine.active_validator_count(), 5);
    }
    #[test]
    fn test_non_validator_cannot_vote() {
        let mut engine = PoAEngine::new(vec!["alice".into()]);
        let result = engine.vote("hacker", "dave".into(), VoteType::Add);
        assert!(result.is_err());
    }
    #[test]
    fn test_prepare_block() {
        let engine = PoAEngine::new(vec!["alice".into()]);
        let mut block = Block::new(1, "0".repeat(64), vec![]);
        engine.prepare_block(&mut block).unwrap();
        assert!(!block.hash.is_empty());
    }
}
