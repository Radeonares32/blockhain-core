use super::{ConsensusEngine, ConsensusError};
use crate::Block;
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
use crate::crypto::{verify_signature, KeyPair};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VoteType {
    Add,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub voter: String,
    pub target: String,
    pub vote_type: VoteType,
    pub signature: Vec<u8>,
}

impl Vote {
    pub fn new(voter: String, target: String, vote_type: VoteType) -> Self {
        Vote {
            voter,
            target,
            vote_type,
            signature: Vec::new(),
        }
    }

    pub fn sign(&mut self, keypair: &KeyPair) {
        let msg = self.signing_bytes();
        let sig = keypair.sign(&msg);
        self.signature = sig.to_vec();
    }

    pub fn verify(&self) -> bool {
        if self.signature.is_empty() {
            return false;
        }

        let public_key_bytes = match hex::decode(&self.voter) {
            Ok(bytes) => {
                if bytes.len() != 32 {
                    return false;
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            }
            Err(_) => return false,
        };
        verify_signature(&self.signing_bytes(), &self.signature, &public_key_bytes).is_ok()
    }

    fn signing_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(self.voter.as_bytes());
        data.extend_from_slice(self.target.as_bytes());
        match self.vote_type {
            VoteType::Add => data.push(0),
            VoteType::Remove => data.push(1),
        }
        data
    }
}

pub struct PoAEngine {
    pub config: PoAConfig,
    pub authorities: Vec<Authority>,
    pub pending_votes: HashMap<String, Vec<(String, VoteType)>>,
    last_producer: Option<String>,
    keypair: Option<KeyPair>,
}
impl PoAEngine {
    pub fn new(validators: Vec<String>, keypair: Option<KeyPair>) -> Self {
        let authorities = validators
            .into_iter()
            .map(|addr| Authority::new(addr))
            .collect();
        PoAEngine {
            config: PoAConfig::default(),
            authorities,
            pending_votes: HashMap::new(),
            last_producer: None,
            keypair,
        }
    }
    pub fn with_config(
        config: PoAConfig,
        validators: Vec<String>,
        keypair: Option<KeyPair>,
    ) -> Self {
        let mut engine = Self::new(validators, keypair);
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

    pub fn vote(&mut self, vote: Vote) -> Result<bool, ConsensusError> {
        if !vote.verify() {
            return Err(ConsensusError("Invalid vote signature".into()));
        }

        let voter = &vote.voter;
        let target = vote.target.clone();
        let vote_type = vote.vote_type;

        if !self
            .authorities
            .iter()
            .any(|a| a.address == *voter && a.active)
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
        let expected_signer_addr = if let Some(expected) = self.expected_proposer(slot) {
            expected.address.clone()
        } else if self.authorities.is_empty() {
            String::new()
        } else {
            return Err(ConsensusError("No valid proposer for this slot".into()));
        };

        if !expected_signer_addr.is_empty() {
            println!(
                "👥 PoA: Block {} should be proposed by: {}",
                slot, expected_signer_addr
            );

            if let Some(keypair) = &self.keypair {
                if keypair.public_key_hex() == expected_signer_addr {
                    block.sign(keypair);
                    println!(
                        "✍️  PoA: Block {} signed by us ({})",
                        block.index, expected_signer_addr
                    );
                } else {
                    println!(
                        "⚠️  PoA: We are notably the proposer (us: {}, expected: {})",
                        keypair.public_key_hex(),
                        expected_signer_addr
                    );
                }
            } else {
                println!("⚠️  PoA: No keypair configured, cannot sign block");
            }
        }

        if block.signature.is_none() {
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

            if !block.verify_signature() {
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
    use crate::crypto::KeyPair;

    #[test]
    fn test_round_robin() {
        let engine = PoAEngine::new(vec!["alice".into(), "bob".into(), "charlie".into()], None);
        assert_eq!(engine.expected_proposer(0).unwrap().address, "alice");
        assert_eq!(engine.expected_proposer(1).unwrap().address, "bob");
        assert_eq!(engine.expected_proposer(2).unwrap().address, "charlie");
        assert_eq!(engine.expected_proposer(3).unwrap().address, "alice");
    }
    #[test]
    fn test_add_remove_authority() {
        let mut engine = PoAEngine::new(vec!["alice".into(), "bob".into()], None);
        assert_eq!(engine.active_validator_count(), 2);
        engine.add_authority("charlie".into());
        assert_eq!(engine.active_validator_count(), 3);
        engine.remove_authority("bob");
        assert_eq!(engine.active_validator_count(), 2);
    }
    #[test]
    fn test_voting() {
        let alice_key = KeyPair::generate().unwrap();
        let bob_key = KeyPair::generate().unwrap();
        let charlie_key = KeyPair::generate().unwrap();
        let eve_key = KeyPair::generate().unwrap();

        let mut engine = PoAEngine::new(
            vec![
                alice_key.public_key_hex(),
                bob_key.public_key_hex(),
                charlie_key.public_key_hex(),
                eve_key.public_key_hex(),
            ],
            None,
        );

        let target = "dave".to_string();

        let mut v1 = Vote::new(alice_key.public_key_hex(), target.clone(), VoteType::Add);
        v1.sign(&alice_key);
        let result = engine.vote(v1).unwrap();
        assert!(!result);

        let mut v2 = Vote::new(bob_key.public_key_hex(), target.clone(), VoteType::Add);
        v2.sign(&bob_key);
        let result = engine.vote(v2).unwrap();
        assert!(!result);

        let mut v3 = Vote::new(charlie_key.public_key_hex(), target.clone(), VoteType::Add);
        v3.sign(&charlie_key);
        let result = engine.vote(v3).unwrap();
        assert!(result);

        assert_eq!(engine.active_validator_count(), 5);
    }

    #[test]
    fn test_non_validator_cannot_vote() {
        let alice_key = KeyPair::generate().unwrap();
        let hacker_key = KeyPair::generate().unwrap();

        let mut engine = PoAEngine::new(vec![alice_key.public_key_hex()], None);

        let mut v = Vote::new(hacker_key.public_key_hex(), "dave".into(), VoteType::Add);
        v.sign(&hacker_key);

        let result = engine.vote(v);
        assert!(result.is_err());
    }
    #[test]
    fn test_prepare_block() {
        let keypair = KeyPair::generate().unwrap();
        let pubkey = keypair.public_key_hex();

        let engine = PoAEngine::new(vec![pubkey.clone()], Some(keypair));
        let mut block = Block::new(1, "0".repeat(64), vec![]);
        engine.prepare_block(&mut block).unwrap();

        assert!(block.signature.is_some());
        assert!(block.verify_signature());
    }
}
