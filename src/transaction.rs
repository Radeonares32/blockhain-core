use crate::crypto::{verify_signature, KeyPair};
use crate::hash::calculate_hash;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub fee: u64,
    pub nonce: u64,
    pub data: Vec<u8>,
    pub timestamp: u128,
    pub hash: String,
    pub signature: Option<Vec<u8>>,
}
impl Transaction {
    pub fn new(from: String, to: String, amount: u64, data: Vec<u8>) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let mut tx = Transaction {
            from,
            to,
            amount,
            fee: 0,
            nonce: 0,
            data,
            timestamp,
            hash: String::new(),
            signature: None,
        };
        tx.hash = tx.calculate_hash();
        tx
    }
    pub fn new_with_fee(
        from: String,
        to: String,
        amount: u64,
        fee: u64,
        nonce: u64,
        data: Vec<u8>,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let mut tx = Transaction {
            from,
            to,
            amount,
            fee,
            nonce,
            data,
            timestamp,
            hash: String::new(),
            signature: None,
        };
        tx.hash = tx.calculate_hash();
        tx
    }
    pub fn genesis() -> Self {
        Transaction {
            from: "genesis".to_string(),
            to: "genesis".to_string(),
            amount: 0,
            fee: 0,
            nonce: 0,
            data: hex::decode("52414445").unwrap(),
            timestamp: 0,
            hash: "genesis".to_string(),
            signature: None,
        }
    }
    pub fn signing_hash(&self) -> [u8; 32] {
        let mut hasher = Sha3_256::new();
        hasher.update(self.from.as_bytes());
        hasher.update(self.to.as_bytes());
        hasher.update(self.amount.to_le_bytes());
        hasher.update(self.fee.to_le_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(&self.data);
        hasher.update(self.timestamp.to_le_bytes());
        hasher.finalize().into()
    }
    pub fn calculate_hash(&self) -> String {
        let data = format!(
            "{}{}{}{}{}{}{}",
            self.from,
            self.to,
            self.amount,
            self.fee,
            self.nonce,
            hex::encode(&self.data),
            self.timestamp
        );
        calculate_hash(data.as_bytes())
    }
    pub fn sign(&mut self, keypair: &KeyPair) {
        let expected_from = keypair.public_key_hex();
        if self.from != expected_from {
            println!(
                "Warning: TX.from ({}) doesn't match keypair pubkey ({})",
                &self.from[..16.min(self.from.len())],
                &expected_from[..16]
            );
        }
        let signing_hash = self.signing_hash();
        let signature = keypair.sign(&signing_hash);
        self.signature = Some(signature.to_vec());
        println!(
            "TX signed: {} -> {} ({} coins)",
            &self.from[..8.min(self.from.len())],
            &self.to[..8.min(self.to.len())],
            self.amount
        );
    }
    pub fn verify(&self) -> bool {
        if self.from == "genesis" {
            return true;
        }
        let signature = match &self.signature {
            Some(s) => s,
            None => {
                println!("TX has no signature");
                return false;
            }
        };
        let public_key = match hex::decode(&self.from) {
            Ok(pk) => pk,
            Err(e) => {
                println!("Invalid from address (not valid hex): {}", e);
                return false;
            }
        };
        if public_key.len() != 32 {
            println!(
                "Invalid public key length: expected 32, got {}",
                public_key.len()
            );
            return false;
        }
        let signing_hash = self.signing_hash();
        match verify_signature(&signing_hash, signature, &public_key) {
            Ok(()) => true,
            Err(e) => {
                println!("TX signature verification failed: {}", e);
                false
            }
        }
    }
    pub fn is_valid(&self) -> bool {
        if !self.verify() {
            return false;
        }
        if self.from == "genesis" {
            return true;
        }
        if self.to.is_empty() {
            println!("TX has empty 'to' address");
            return false;
        }
        true
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
    pub fn total_cost(&self) -> u64 {
        self.amount.saturating_add(self.fee)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_transaction_creation() {
        let tx = Transaction::new("alice".into(), "bob".into(), 100, vec![]);
        assert_eq!(tx.amount, 100);
        assert!(tx.signature.is_none());
    }
    #[test]
    fn test_transaction_with_fee() {
        let tx = Transaction::new_with_fee("alice".into(), "bob".into(), 100, 5, 1, vec![]);
        assert_eq!(tx.fee, 5);
        assert_eq!(tx.nonce, 1);
        assert_eq!(tx.total_cost(), 105);
    }
    #[test]
    fn test_genesis_transaction() {
        let genesis = Transaction::genesis();
        assert!(genesis.verify());
        assert!(genesis.is_valid());
    }
    #[test]
    fn test_unsigned_transaction_is_invalid() {
        let tx = Transaction::new("not_genesis".into(), "bob".into(), 100, vec![]);
        assert!(!tx.verify());
    }
    #[test]
    fn test_sign_and_verify() {
        let keypair = KeyPair::generate().unwrap();
        let mut tx = Transaction::new_with_fee(
            keypair.public_key_hex(),
            "recipient".into(),
            50,
            1,
            0,
            vec![],
        );
        assert!(!tx.verify());
        tx.sign(&keypair);
        assert!(tx.verify());
        assert!(tx.is_valid());
    }
    #[test]
    fn test_wrong_keypair_fails() {
        let keypair1 = KeyPair::generate().unwrap();
        let keypair2 = KeyPair::generate().unwrap();
        let mut tx = Transaction::new_with_fee(
            keypair1.public_key_hex(),
            "recipient".into(),
            50,
            1,
            0,
            vec![],
        );
        let signing_hash = tx.signing_hash();
        let wrong_signature = keypair2.sign(&signing_hash);
        tx.signature = Some(wrong_signature.to_vec());
        assert!(!tx.verify());
    }
    #[test]
    fn test_modified_tx_fails() {
        let keypair = KeyPair::generate().unwrap();
        let mut tx = Transaction::new_with_fee(
            keypair.public_key_hex(),
            "recipient".into(),
            50,
            1,
            0,
            vec![],
        );
        tx.sign(&keypair);
        assert!(tx.verify());
        tx.amount = 1000;
        assert!(!tx.verify());
    }
    #[test]
    fn test_signing_hash_deterministic() {
        let tx = Transaction::new_with_fee("from".into(), "to".into(), 100, 1, 5, b"data".to_vec());
        let hash1 = tx.signing_hash();
        let hash2 = tx.signing_hash();
        assert_eq!(hash1, hash2);
    }
}
