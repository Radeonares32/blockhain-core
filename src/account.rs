use crate::storage::Storage;
use crate::Transaction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
pub const MIN_TX_FEE: u64 = 1;
pub const GENESIS_BALANCE: u64 = 1_000_000_000;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub public_key: String,
    pub balance: u64,
    pub nonce: u64,
}
impl Account {
    pub fn new(public_key: String) -> Self {
        Account {
            public_key,
            balance: 0,
            nonce: 0,
        }
    }
    pub fn with_balance(public_key: String, balance: u64) -> Self {
        Account {
            public_key,
            balance,
            nonce: 0,
        }
    }
}
pub struct AccountState {
    accounts: HashMap<String, Account>,
    storage: Option<Storage>,
}
impl AccountState {
    pub fn new() -> Self {
        AccountState {
            accounts: HashMap::new(),
            storage: None,
        }
    }
    pub fn with_storage(storage: Storage) -> Self {
        let mut state = AccountState {
            accounts: HashMap::new(),
            storage: Some(storage),
        };
        if let Err(e) = state.load_from_storage() {
            println!("Could not load account state: {}", e);
        }
        state
    }
    pub fn init_genesis(&mut self, genesis_pubkey: &str) {
        let account = Account::with_balance(genesis_pubkey.to_string(), GENESIS_BALANCE);
        self.accounts.insert(genesis_pubkey.to_string(), account);
        println!("Genesis account created: {} coins", GENESIS_BALANCE);
    }
    pub fn get_balance(&self, public_key: &str) -> u64 {
        self.accounts
            .get(public_key)
            .map(|a| a.balance)
            .unwrap_or(0)
    }
    pub fn get_nonce(&self, public_key: &str) -> u64 {
        self.accounts.get(public_key).map(|a| a.nonce).unwrap_or(0)
    }
    fn get_or_create(&mut self, public_key: &str) -> &mut Account {
        if !self.accounts.contains_key(public_key) {
            self.accounts
                .insert(public_key.to_string(), Account::new(public_key.to_string()));
        }
        self.accounts.get_mut(public_key).unwrap()
    }
    pub fn validate_transaction(&self, tx: &Transaction) -> Result<(), String> {
        if tx.from == "genesis" {
            return Ok(());
        }
        if !tx.verify() {
            return Err("Invalid signature".into());
        }
        if tx.fee < MIN_TX_FEE {
            return Err(format!("Fee too low: {} < {}", tx.fee, MIN_TX_FEE));
        }
        let expected_nonce = self.get_nonce(&tx.from);
        if tx.nonce != expected_nonce {
            return Err(format!(
                "Invalid nonce: expected {}, got {}",
                expected_nonce, tx.nonce
            ));
        }
        let balance = self.get_balance(&tx.from);
        let total_cost = tx.total_cost();
        if balance < total_cost {
            return Err(format!(
                "Insufficient balance: {} < {} (amount: {}, fee: {})",
                balance, total_cost, tx.amount, tx.fee
            ));
        }
        Ok(())
    }
    pub fn apply_transaction(&mut self, tx: &Transaction) -> Result<(), String> {
        if tx.from == "genesis" {
            return Ok(());
        }
        let total_cost = tx.total_cost();
        {
            let from_account = self.get_or_create(&tx.from);
            if from_account.balance < total_cost {
                return Err("Insufficient balance".into());
            }
            from_account.balance -= total_cost;
            from_account.nonce += 1;
        }
        {
            let to_account = self.get_or_create(&tx.to);
            to_account.balance += tx.amount;
        }
        Ok(())
    }
    pub fn apply_block(&mut self, transactions: &[Transaction], block_producer: Option<&str>) {
        let mut total_fees: u64 = 0;
        for tx in transactions {
            if tx.from == "genesis" {
                continue;
            }
            if let Err(e) = self.apply_transaction(tx) {
                println!("TX apply failed: {}", e);
                continue;
            }
            total_fees += tx.fee;
        }
        if let Some(producer) = block_producer {
            if total_fees > 0 {
                let producer_account = self.get_or_create(producer);
                producer_account.balance += total_fees;
                println!(
                    "Block producer {} received {} in fees",
                    &producer[..16.min(producer.len())],
                    total_fees
                );
            }
        }
    }
    pub fn add_balance(&mut self, public_key: &str, amount: u64) {
        let account = self.get_or_create(public_key);
        account.balance += amount;
    }
    pub fn save_to_storage(&self) -> Result<(), String> {
        let storage = match &self.storage {
            Some(s) => s,
            None => return Ok(()),
        };
        let data = serde_json::to_vec(&self.accounts)
            .map_err(|e| format!("Serialization error: {}", e))?;
        storage
            .db()
            .insert("ACCOUNT_STATE", data)
            .map_err(|e| format!("Storage error: {}", e))?;
        storage
            .db()
            .flush()
            .map_err(|e| format!("Flush error: {}", e))?;
        Ok(())
    }
    fn load_from_storage(&mut self) -> Result<(), String> {
        let storage = match &self.storage {
            Some(s) => s,
            None => return Ok(()),
        };
        if let Ok(Some(data)) = storage.db().get("ACCOUNT_STATE") {
            let accounts: HashMap<String, Account> = serde_json::from_slice(&data)
                .map_err(|e| format!("Deserialization error: {}", e))?;
            self.accounts = accounts;
            println!("Loaded {} accounts from storage", self.accounts.len());
        }
        Ok(())
    }
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }
    #[allow(dead_code)]
    pub fn print_balances(&self) {
        println!("Account Balances:");
        for (pubkey, account) in &self.accounts {
            println!(
                "  {}...  balance: {}, nonce: {}",
                &pubkey[..16.min(pubkey.len())],
                account.balance,
                account.nonce
            );
        }
    }
}
impl Default for AccountState {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;
    #[test]
    fn test_new_account() {
        let account = Account::new("pubkey123".into());
        assert_eq!(account.balance, 0);
        assert_eq!(account.nonce, 0);
    }
    #[test]
    fn test_account_with_balance() {
        let account = Account::with_balance("pubkey123".into(), 1000);
        assert_eq!(account.balance, 1000);
    }
    #[test]
    fn test_account_state_balance() {
        let mut state = AccountState::new();
        state.add_balance("alice", 500);
        assert_eq!(state.get_balance("alice"), 500);
        assert_eq!(state.get_balance("bob"), 0);
    }
    #[test]
    fn test_transfer() {
        let alice = KeyPair::generate().unwrap();
        let bob = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 1000);
        let mut tx = Transaction::new_with_fee(
            alice.public_key_hex(),
            bob.public_key_hex(),
            100,
            5,
            0,
            vec![],
        );
        tx.sign(&alice);
        assert!(state.validate_transaction(&tx).is_ok());
        state.apply_transaction(&tx).unwrap();
        assert_eq!(state.get_balance(&alice.public_key_hex()), 895);
        assert_eq!(state.get_balance(&bob.public_key_hex()), 100);
        assert_eq!(state.get_nonce(&alice.public_key_hex()), 1);
    }
    #[test]
    fn test_insufficient_balance() {
        let alice = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 50);
        let mut tx =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 100, 1, 0, vec![]);
        tx.sign(&alice);
        assert!(state.validate_transaction(&tx).is_err());
    }
    #[test]
    fn test_wrong_nonce() {
        let alice = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 1000);
        let mut tx =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 100, 1, 5, vec![]);
        tx.sign(&alice);
        let result = state.validate_transaction(&tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("nonce"));
    }
    #[test]
    fn test_replay_protection() {
        let alice = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 1000);
        let mut tx1 =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 50, 1, 0, vec![]);
        tx1.sign(&alice);
        assert!(state.validate_transaction(&tx1).is_ok());
        state.apply_transaction(&tx1).unwrap();
        assert!(state.validate_transaction(&tx1).is_err());
        let mut tx2 =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 50, 1, 1, vec![]);
        tx2.sign(&alice);
        assert!(state.validate_transaction(&tx2).is_ok());
    }
    #[test]
    fn test_fee_too_low() {
        let alice = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 1000);
        let mut tx =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 100, 0, 0, vec![]);
        tx.sign(&alice);
        let result = state.validate_transaction(&tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Fee"));
    }
}
