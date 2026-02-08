use crate::{Block, Transaction};
use serde::{Deserialize, Serialize};

pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;
pub const MAX_BLOCK_SIZE: usize = 1 * 1024 * 1024;
pub const MAX_TX_SIZE: usize = 100 * 1024;
pub const MAX_CHAIN_SYNC_BLOCKS: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub enum MessageError {
    TooLarge(usize),
    ParseError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    Block(Block),
    Transaction(Transaction),
    GetBlocks,
    Chain(Vec<Block>),
}
impl NetworkMessage {
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
    pub fn from_bytes_validated(bytes: &[u8]) -> Result<Self, MessageError> {
        if bytes.len() > MAX_MESSAGE_SIZE {
            return Err(MessageError::TooLarge(bytes.len()));
        }
        serde_json::from_slice(bytes).map_err(|e| MessageError::ParseError(e.to_string()))
    }
    pub fn validate_block_size(block: &Block) -> Result<(), MessageError> {
        let size = serde_json::to_vec(block).unwrap_or_default().len();
        if size > MAX_BLOCK_SIZE {
            return Err(MessageError::TooLarge(size));
        }
        Ok(())
    }
    pub fn validate_tx_size(tx: &Transaction) -> Result<(), MessageError> {
        let size = serde_json::to_vec(tx).unwrap_or_default().len();
        if size > MAX_TX_SIZE {
            return Err(MessageError::TooLarge(size));
        }
        Ok(())
    }
}
