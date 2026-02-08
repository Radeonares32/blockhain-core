use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SlashingType {
    DoubleSign,
    Downtime,
    InvalidBlock,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvidence {
    pub offense_type: SlashingType,
    pub validator: String,
    pub height: u64,
    pub block_hash_1: Option<String>,
    pub block_hash_2: Option<String>,
    pub signature_1: Option<Vec<u8>>,
    pub signature_2: Option<Vec<u8>>,
    pub timestamp: u128,
    pub reporter: String,
}
impl SlashingEvidence {
    pub fn double_sign(
        validator: String,
        height: u64,
        block_hash_1: String,
        block_hash_2: String,
        signature_1: Vec<u8>,
        signature_2: Vec<u8>,
        reporter: String,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        SlashingEvidence {
            offense_type: SlashingType::DoubleSign,
            validator,
            height,
            block_hash_1: Some(block_hash_1),
            block_hash_2: Some(block_hash_2),
            signature_1: Some(signature_1),
            signature_2: Some(signature_2),
            timestamp,
            reporter,
        }
    }
    pub fn downtime(validator: String, height: u64, reporter: String) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        SlashingEvidence {
            offense_type: SlashingType::Downtime,
            validator,
            height,
            block_hash_1: None,
            block_hash_2: None,
            signature_1: None,
            signature_2: None,
            timestamp,
            reporter,
        }
    }
    pub fn verify_double_sign(&self) -> Result<(), String> {
        if self.offense_type != SlashingType::DoubleSign {
            return Err("Wrong offense type".to_string());
        }
        let hash1 = self.block_hash_1.as_ref().ok_or("Missing block_hash_1")?;
        let hash2 = self.block_hash_2.as_ref().ok_or("Missing block_hash_2")?;
        if hash1 == hash2 {
            return Err("Block hashes are identical".to_string());
        }
        let sig1 = self.signature_1.as_ref().ok_or("Missing signature_1")?;
        let sig2 = self.signature_2.as_ref().ok_or("Missing signature_2")?;
        if sig1 == sig2 {
            return Err("Signatures are identical".to_string());
        }
        let pubkey_bytes =
            hex::decode(&self.validator).map_err(|e| format!("Invalid validator pubkey: {}", e))?;
        if pubkey_bytes.len() != 32 {
            return Err("Invalid validator pubkey length".to_string());
        }
        Ok(())
    }
    pub fn slash_amount(&self, stake: u64) -> u64 {
        match self.offense_type {
            SlashingType::DoubleSign => stake,
            SlashingType::Downtime => stake / 10,
            SlashingType::InvalidBlock => stake / 2,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_double_sign_evidence() {
        let evidence = SlashingEvidence::double_sign(
            "abc123".repeat(5),
            100,
            "hash1".to_string(),
            "hash2".to_string(),
            vec![1, 2, 3],
            vec![4, 5, 6],
            "reporter_pubkey".to_string(),
        );
        assert_eq!(evidence.offense_type, SlashingType::DoubleSign);
        assert_eq!(evidence.height, 100);
        assert!(evidence.block_hash_1.is_some());
        assert!(evidence.block_hash_2.is_some());
    }
    #[test]
    fn test_slash_amounts() {
        let evidence =
            SlashingEvidence::downtime("validator".to_string(), 50, "reporter".to_string());
        assert_eq!(evidence.slash_amount(1000), 100);
    }
    #[test]
    fn test_verify_double_sign_requires_different_hashes() {
        let evidence = SlashingEvidence {
            offense_type: SlashingType::DoubleSign,
            validator: "a".repeat(64),
            height: 100,
            block_hash_1: Some("hash".to_string()),
            block_hash_2: Some("hash".to_string()),
            signature_1: Some(vec![1, 2, 3]),
            signature_2: Some(vec![4, 5, 6]),
            timestamp: 0,
            reporter: "reporter".to_string(),
        };
        let result = evidence.verify_double_sign();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("identical"));
    }
}
