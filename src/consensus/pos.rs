use super::{ConsensusEngine, ConsensusError};
use crate::Block;
use sha3::{Digest, Sha3_256};
use std::collections::HashMap;
#[derive(Debug, Clone)]
pub struct PoSConfig {
    pub min_stake: u64,
    pub slot_duration: u64,
    pub epoch_length: u64,
    pub annual_reward_rate: f64,
    pub slashing_penalty: f64,
    pub double_sign_penalty: f64,
}
impl Default for PoSConfig {
    fn default() -> Self {
        PoSConfig {
            min_stake: 1000,
            slot_duration: 6,
            epoch_length: 32,
            annual_reward_rate: 0.05,
            slashing_penalty: 0.10,
            double_sign_penalty: 0.50,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Validator {
    pub pubkey: String,
    pub stake: u64,
    pub active: bool,
    pub slashed: bool,
    pub last_proposed_block: Option<u64>,
}
impl Validator {
    pub fn new(pubkey: String, stake: u64) -> Self {
        Validator {
            pubkey,
            stake,
            active: true,
            slashed: false,
            last_proposed_block: None,
        }
    }
    pub fn effective_stake(&self) -> u64 {
        if self.slashed {
            0
        } else {
            self.stake
        }
    }
}
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SlashingEvidence {
    pub validator: String,
    pub block1_hash: String,
    pub block2_hash: String,
    pub signature1: Vec<u8>,
    pub signature2: Vec<u8>,
    pub slot: u64,
    pub timestamp: u128,
}
impl SlashingEvidence {
    pub fn new(
        validator: String,
        block1_hash: String,
        block2_hash: String,
        signature1: Vec<u8>,
        signature2: Vec<u8>,
        slot: u64,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        SlashingEvidence {
            validator,
            block1_hash,
            block2_hash,
            signature1,
            signature2,
            slot,
            timestamp,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub block_index: u64,
    pub block_hash: String,
    pub timestamp: u128,
}
use crate::crypto::KeyPair;

pub struct PoSEngine {
    pub config: PoSConfig,
    pub validators: HashMap<String, Validator>,
    total_stake: u64,
    seen_blocks: HashMap<(String, u64), String>,
    slashing_evidence: Vec<SlashingEvidence>,
    checkpoints: Vec<Checkpoint>,
    keypair: Option<KeyPair>,
}
impl PoSEngine {
    pub fn new(min_stake: u64, keypair: Option<KeyPair>) -> Self {
        PoSEngine {
            config: PoSConfig {
                min_stake,
                ..Default::default()
            },
            validators: HashMap::new(),
            total_stake: 0,
            seen_blocks: HashMap::new(),
            slashing_evidence: Vec::new(),
            checkpoints: Vec::new(),
            keypair,
        }
    }
    pub fn with_config(config: PoSConfig, keypair: Option<KeyPair>) -> Self {
        PoSEngine {
            config,
            validators: HashMap::new(),
            total_stake: 0,
            seen_blocks: HashMap::new(),
            slashing_evidence: Vec::new(),
            checkpoints: Vec::new(),
            keypair,
        }
    }
    pub fn add_stake(&mut self, pubkey: String, amount: u64) -> Result<(), ConsensusError> {
        if amount < self.config.min_stake && !self.validators.contains_key(&pubkey) {
            return Err(ConsensusError(format!(
                "Minimum stake {} required, got {}",
                self.config.min_stake, amount
            )));
        }
        let validator = self
            .validators
            .entry(pubkey.clone())
            .or_insert_with(|| Validator::new(pubkey, 0));
        validator.stake += amount;
        validator.active = true;
        self.total_stake += amount;
        println!(
            "🥩 Stake added: {} coins, total validators: {}",
            amount,
            self.validators.len()
        );
        Ok(())
    }
    pub fn withdraw_stake(&mut self, pubkey: &str, amount: u64) -> Result<(), ConsensusError> {
        let validator = self
            .validators
            .get_mut(pubkey)
            .ok_or_else(|| ConsensusError("Validator not found".into()))?;
        if validator.stake < amount {
            return Err(ConsensusError("Insufficient stake".into()));
        }
        validator.stake -= amount;
        self.total_stake -= amount;
        if validator.stake < self.config.min_stake {
            validator.active = false;
        }
        Ok(())
    }
    pub fn slash(&mut self, pubkey: &str) -> Result<u64, ConsensusError> {
        let validator = self
            .validators
            .get_mut(pubkey)
            .ok_or_else(|| ConsensusError("Validator not found".into()))?;
        let penalty = (validator.stake as f64 * self.config.slashing_penalty) as u64;
        validator.stake = validator.stake.saturating_sub(penalty);
        validator.slashed = true;
        validator.active = false;
        self.total_stake -= penalty;
        println!("⚠️  Validator slashed: {} lost {} coins", pubkey, penalty);
        Ok(penalty)
    }
    pub fn slash_double_sign(
        &mut self,
        pubkey: &str,
        evidence: SlashingEvidence,
    ) -> Result<u64, ConsensusError> {
        let validator = self
            .validators
            .get_mut(pubkey)
            .ok_or_else(|| ConsensusError("Validator not found".into()))?;
        let penalty = (validator.stake as f64 * self.config.double_sign_penalty) as u64;
        validator.stake = validator.stake.saturating_sub(penalty);
        validator.slashed = true;
        validator.active = false;
        self.total_stake -= penalty;
        self.slashing_evidence.push(evidence);
        println!(
            "🚨 DOUBLE-SIGN DETECTED! Validator {} lost {} coins (50% stake)",
            pubkey, penalty
        );
        Ok(penalty)
    }
    pub fn check_and_record_block(
        &mut self,
        producer: &str,
        slot: u64,
        block_hash: &str,
    ) -> Result<(), ConsensusError> {
        let key = (producer.to_string(), slot);
        if let Some(existing_hash) = self.seen_blocks.get(&key) {
            if existing_hash != block_hash {
                let evidence = SlashingEvidence::new(
                    producer.to_string(),
                    existing_hash.clone(),
                    block_hash.to_string(),
                    vec![],
                    vec![],
                    slot,
                );
                println!(
                    "🚨 DOUBLE-SIGN: {} signed two blocks for slot {}!",
                    producer, slot
                );
                println!(
                    "   Block 1: {}...",
                    &existing_hash[..existing_hash.len().min(16)]
                );
                println!("   Block 2: {}...", &block_hash[..block_hash.len().min(16)]);
                self.slash_double_sign(producer, evidence)?;
                return Err(ConsensusError("Double-sign detected and slashed".into()));
            }
        } else {
            self.seen_blocks.insert(key, block_hash.to_string());
        }
        Ok(())
    }
    pub fn add_checkpoint(&mut self, block: &Block) {
        let checkpoint = Checkpoint {
            block_index: block.index,
            block_hash: block.hash.clone(),
            timestamp: block.timestamp,
        };
        self.checkpoints.push(checkpoint);
        println!(
            "📍 Checkpoint added at block {} (every {} slots)",
            block.index, self.config.epoch_length
        );
    }
    pub fn is_before_checkpoint(&self, block: &Block) -> bool {
        if let Some(last_cp) = self.checkpoints.last() {
            block.index < last_cp.block_index
        } else {
            false
        }
    }
    pub fn select_validator(&self, previous_hash: &str, slot: u64) -> Option<&Validator> {
        if self.total_stake == 0 {
            return None;
        }
        let mut hasher = Sha3_256::new();
        hasher.update(previous_hash.as_bytes());
        hasher.update(slot.to_le_bytes());
        let hash = hasher.finalize();
        let random_value = u64::from_le_bytes(hash[0..8].try_into().unwrap());
        let selection_point = random_value % self.total_stake;
        let mut cumulative: u64 = 0;
        let mut active_validators: Vec<_> = self
            .validators
            .values()
            .filter(|v| v.active && !v.slashed)
            .collect();
        active_validators.sort_by(|a, b| a.pubkey.cmp(&b.pubkey));
        for validator in active_validators {
            cumulative += validator.effective_stake();
            if selection_point < cumulative {
                return Some(validator);
            }
        }
        None
    }
    pub fn is_validator(&self, pubkey: &str) -> bool {
        self.validators.get(pubkey).map_or(false, |v| {
            v.active && !v.slashed && v.stake >= self.config.min_stake
        })
    }
    #[allow(dead_code)]
    fn calculate_reward(&self, validator_stake: u64) -> u64 {
        let slots_per_year = 365 * 24 * 60 * 60 / self.config.slot_duration;
        let reward = (validator_stake as f64 * self.config.annual_reward_rate
            / slots_per_year as f64) as u64;
        reward.max(1)
    }

    pub fn get_slashing_evidence(&self) -> &[SlashingEvidence] {
        &self.slashing_evidence
    }
    pub fn get_checkpoints(&self) -> &[Checkpoint] {
        &self.checkpoints
    }
    pub fn serialize_state(&self) -> Result<Vec<u8>, String> {
        let state = serde_json::json!({
            "validators": self.validators.iter().map(|(k, v)| {
                serde_json::json!({
                    "pubkey": k,
                    "stake": v.stake,
                    "active": v.active,
                    "slashed": v.slashed,
                })
            }).collect::<Vec<_>>(),
            "total_stake": self.total_stake,
            "checkpoints": self.checkpoints.iter().map(|c| {
                serde_json::json!({
                    "block_index": c.block_index,
                    "block_hash": c.block_hash,
                    "timestamp": c.timestamp,
                })
            }).collect::<Vec<_>>(),
        });
        serde_json::to_vec(&state).map_err(|e| format!("Serialization error: {}", e))
    }
    pub fn save_state(&self, db: &sled::Db) -> Result<(), String> {
        let data = self.serialize_state()?;
        db.insert("POS_STATE", data)
            .map_err(|e| format!("DB insert error: {}", e))?;
        db.flush().map_err(|e| format!("DB flush error: {}", e))?;
        println!(
            "💾 PoS state saved: {} validators, {} total stake",
            self.validators.len(),
            self.total_stake
        );
        Ok(())
    }
    pub fn load_state(&mut self, db: &sled::Db) -> Result<(), String> {
        let data = match db.get("POS_STATE") {
            Ok(Some(d)) => d,
            Ok(None) => {
                println!("📭 No saved PoS state found, starting fresh");
                return Ok(());
            }
            Err(e) => return Err(format!("DB read error: {}", e)),
        };
        let state: serde_json::Value =
            serde_json::from_slice(&data).map_err(|e| format!("Deserialization error: {}", e))?;
        if let Some(validators) = state.get("validators").and_then(|v| v.as_array()) {
            for v in validators {
                let pubkey = v.get("pubkey").and_then(|p| p.as_str()).unwrap_or("");
                let stake = v.get("stake").and_then(|s| s.as_u64()).unwrap_or(0);
                let active = v.get("active").and_then(|a| a.as_bool()).unwrap_or(true);
                let slashed = v.get("slashed").and_then(|s| s.as_bool()).unwrap_or(false);
                let validator = Validator {
                    pubkey: pubkey.to_string(),
                    stake,
                    active,
                    slashed,
                    last_proposed_block: None,
                };
                self.validators.insert(pubkey.to_string(), validator);
            }
        }
        if let Some(total) = state.get("total_stake").and_then(|t| t.as_u64()) {
            self.total_stake = total;
        }
        if let Some(checkpoints) = state.get("checkpoints").and_then(|c| c.as_array()) {
            for cp in checkpoints {
                let block_index = cp.get("block_index").and_then(|i| i.as_u64()).unwrap_or(0);
                let block_hash = cp
                    .get("block_hash")
                    .and_then(|h| h.as_str())
                    .unwrap_or("")
                    .to_string();
                let timestamp = cp.get("timestamp").and_then(|t| t.as_u64()).unwrap_or(0) as u128;
                self.checkpoints.push(Checkpoint {
                    block_index,
                    block_hash,
                    timestamp,
                });
            }
        }
        println!(
            "✅ PoS state loaded: {} validators, {} total stake, {} checkpoints",
            self.validators.len(),
            self.total_stake,
            self.checkpoints.len()
        );
        Ok(())
    }
}
impl ConsensusEngine for PoSEngine {
    fn prepare_block(&self, block: &mut Block) -> Result<(), ConsensusError> {
        let slot = block.index;
        println!("🥩 PoS: Preparing block for slot {}", slot);
        if !self.validators.is_empty() {
            if let Some(validator) = self.select_validator(&block.previous_hash, slot) {
                let pubkey = &validator.pubkey;
                println!(
                    "🥩 PoS: Selected validator: {} (stake: {})",
                    &pubkey[..16.min(pubkey.len())],
                    validator.stake
                );

                if let Some(keypair) = &self.keypair {
                    if keypair.public_key_hex() == *pubkey {
                        block.sign(keypair);

                        block.add_stake_proof(block.signature.clone().unwrap());
                        println!(
                            "✍️  PoS: Block {} signed by selected validator {}",
                            block.index,
                            &pubkey[..16.min(pubkey.len())]
                        );
                    } else {
                        println!(
                            "⚠️  PoS: We are not the selected validator (us: {}, selected: {})",
                            keypair.public_key_hex(),
                            pubkey
                        );
                    }
                } else {
                    println!("⚠️  PoS: No keypair configured, cannot sign block");
                }

                if block.signature.is_none() {
                    block.producer = Some(pubkey.clone());
                    block.hash = block.calculate_hash();
                }
            } else {
                return Err(ConsensusError("No active validator available".into()));
            }
        } else {
            println!("⚠️  PoS: No validators registered, skipping stake proof");
            block.hash = block.calculate_hash();
        }
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
        if self.is_before_checkpoint(block) {
            return Err(ConsensusError(
                "Block is before last checkpoint (possible long-range attack)".into(),
            ));
        }
        if !self.validators.is_empty() {
            let producer = block
                .producer
                .as_ref()
                .ok_or_else(|| ConsensusError("Block has no producer".into()))?;
            let expected = self
                .select_validator(&block.previous_hash, block.index)
                .ok_or_else(|| ConsensusError("No validator for this slot".into()))?;
            if producer != &expected.pubkey {
                return Err(ConsensusError(format!(
                    "Wrong validator. Expected: {}, Got: {}",
                    &expected.pubkey[..16.min(expected.pubkey.len())],
                    &producer[..16.min(producer.len())]
                )));
            }

            if !block.verify_signature() {
                return Err(ConsensusError("Invalid block signature".into()));
            }

            match &block.stake_proof {
                Some(proof) => {
                    if let Some(sig) = &block.signature {
                        if proof != sig {
                            return Err(ConsensusError(
                                "Stake proof does not match signature".into(),
                            ));
                        }
                    }
                }
                None => {
                    return Err(ConsensusError("Missing stake proof".into()));
                }
            }

            if let Some(evidences) = &block.slashing_evidence {
                for (i, evidence) in evidences.iter().enumerate() {
                    let validator_bytes = match hex::decode(&evidence.validator) {
                        Ok(bytes) => bytes,
                        Err(_) => {
                            return Err(ConsensusError(format!(
                                "Invalid validator hex in slashing evidence #{}",
                                i
                            )))
                        }
                    };

                    if crate::crypto::verify_signature(
                        evidence.block1_hash.as_bytes(),
                        &evidence.signature1,
                        &validator_bytes,
                    )
                    .is_err()
                    {
                        return Err(ConsensusError(format!(
                            "Invalid signature1 in slashing evidence #{}",
                            i
                        )));
                    }
                    if crate::crypto::verify_signature(
                        evidence.block2_hash.as_bytes(),
                        &evidence.signature2,
                        &validator_bytes,
                    )
                    .is_err()
                    {
                        return Err(ConsensusError(format!(
                            "Invalid signature2 in slashing evidence #{}",
                            i
                        )));
                    }

                    if evidence.block1_hash == evidence.block2_hash {
                        return Err(ConsensusError(format!(
                            "Invalid evidence #{}: hashes are identical",
                            i
                        )));
                    }

                    if !self.validators.contains_key(&evidence.validator) {
                        println!(
                            "⚠️  Warning: Slashing evidence for unknown validator {}",
                            evidence.validator
                        );
                    } else {
                        println!(
                            "⚖️  Valid Slashing Evidence found for validator {}",
                            evidence.validator
                        );
                    }
                }
            }

            println!(
                "✅ PoS: Block {} validated (producer: {}, stake: {})",
                block.index,
                &producer[..16.min(producer.len())],
                expected.stake
            );
        } else {
            if block.hash != block.calculate_hash() {
                return Err(ConsensusError("Invalid block hash".into()));
            }
        }
        Ok(())
    }
    fn consensus_type(&self) -> &'static str {
        "PoS"
    }
    fn info(&self) -> String {
        let active_count = self
            .validators
            .values()
            .filter(|v| v.active && !v.slashed)
            .count();
        format!(
            "PoS (min_stake: {}, validators: {}, total_stake: {}, checkpoints: {})",
            self.config.min_stake,
            active_count,
            self.total_stake,
            self.checkpoints.len()
        )
    }
    fn select_best_chain<'a>(&self, chains: &[&'a [Block]]) -> Option<&'a [Block]> {
        if chains.is_empty() {
            return None;
        }
        chains.iter().max_by_key(|c| c.len()).copied()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;

    #[test]
    fn test_stake_management() {
        let mut engine = PoSEngine::new(100, None);
        engine.add_stake("validator1".into(), 500).unwrap();
        engine.add_stake("validator2".into(), 300).unwrap();
        assert_eq!(engine.total_stake, 800);
        assert!(engine.is_validator("validator1"));
        assert!(engine.is_validator("validator2"));
    }
    #[test]
    fn test_minimum_stake() {
        let mut engine = PoSEngine::new(1000, None);
        let result = engine.add_stake("weak_validator".into(), 500);
        assert!(result.is_err());
    }
    #[test]
    fn test_validator_selection() {
        let mut engine = PoSEngine::new(100, None);
        engine.add_stake("alice".into(), 1000).unwrap();
        engine.add_stake("bob".into(), 500).unwrap();
        engine.add_stake("charlie".into(), 500).unwrap();
        let prev_hash = "abc123";
        let v1 = engine.select_validator(prev_hash, 1);
        let v2 = engine.select_validator(prev_hash, 1);
        assert!(v1.is_some());
        assert_eq!(v1.unwrap().pubkey, v2.unwrap().pubkey);
    }
    #[test]
    fn test_slashing() {
        let mut engine = PoSEngine::new(100, None);
        engine.add_stake("bad_actor".into(), 1000).unwrap();
        let penalty = engine.slash("bad_actor").unwrap();
        assert_eq!(penalty, 100);
        assert!(!engine.is_validator("bad_actor"));
    }
    #[test]
    fn test_weighted_selection() {
        let mut engine = PoSEngine::new(100, None);
        engine.add_stake("alice".into(), 8000).unwrap();
        engine.add_stake("bob".into(), 2000).unwrap();
        let mut alice_count = 0;
        let mut bob_count = 0;
        for slot in 0..100 {
            if let Some(v) = engine.select_validator("test_hash", slot) {
                if v.pubkey == "alice" {
                    alice_count += 1;
                } else {
                    bob_count += 1;
                }
            }
        }
        assert!(alice_count > bob_count);
    }
    #[test]
    fn test_double_sign_detection() {
        let mut engine = PoSEngine::new(100, None);
        engine.add_stake("validator1".into(), 1000).unwrap();
        let result1 = engine.check_and_record_block("validator1", 10, "hash1");
        assert!(result1.is_ok());
        let result2 = engine.check_and_record_block("validator1", 10, "hash1");
        assert!(result2.is_ok());
        let result3 = engine.check_and_record_block("validator1", 10, "hash2");
        assert!(result3.is_err());
        assert!(!engine.is_validator("validator1"));
        assert_eq!(engine.get_slashing_evidence().len(), 1);
    }
    #[test]
    fn test_double_sign_penalty() {
        let mut engine = PoSEngine::new(100, None);
        engine.add_stake("bad_validator".into(), 1000).unwrap();
        engine
            .check_and_record_block("bad_validator", 5, "block_a")
            .unwrap();
        let _ = engine.check_and_record_block("bad_validator", 5, "block_b");
        let validator = engine.validators.get("bad_validator").unwrap();
        assert_eq!(validator.stake, 500);
    }
    #[test]
    fn test_checkpoint() {
        let mut engine = PoSEngine::new(100, None);
        let block = Block::new(32, "prev".into(), vec![]);
        engine.add_checkpoint(&block);
        assert_eq!(engine.get_checkpoints().len(), 1);
        assert_eq!(engine.get_checkpoints()[0].block_index, 32);
    }
    #[test]
    fn test_pos_signing() {
        let keypair = KeyPair::generate().unwrap();
        let pubkey = keypair.public_key_hex();

        let mut engine = PoSEngine::new(100, Some(keypair));
        engine.add_stake(pubkey.clone(), 1000).unwrap();

        let mut block = Block::new(1, "0".repeat(64), vec![]);

        engine.prepare_block(&mut block).unwrap();

        assert!(block.signature.is_some());
        assert!(block.verify_signature());
    }
}
