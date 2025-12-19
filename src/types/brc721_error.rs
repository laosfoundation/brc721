use thiserror::Error;

use super::Brc721Command;

#[derive(Debug, Error, PartialEq)]
pub enum Brc721Error {
    #[error("script too short")]
    ScriptTooShort,
    #[error("invalid payload")]
    InvalidPayload,
    #[error("invalid script length: expected {0} actual {1}")]
    InvalidLength(usize, usize),
    #[error("unknown command: got {0}")]
    UnknownCommand(u8),
    #[error("unsupported command: {cmd:?}")]
    UnsupportedCommand { cmd: Brc721Command },
    #[error("invalid rebase flag: {0}")]
    InvalidRebaseFlag(u8),
    #[error("slot number too large for 96 bits: {0}")]
    InvalidSlotNumber(u128),
    #[error("group count must be at least 1, got {0}")]
    InvalidGroupCount(u8),
    #[error("output index must be at least 1, got {0}")]
    InvalidOutputIndex(u8),
    #[error("range count must be at least 1, got {0}")]
    InvalidRangeCount(u8),
    #[error("slot range start {0} is greater than end {1}")]
    InvalidSlotRange(u128, u128),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("RPC error: {0}")]
    RpcError(String),
    #[error("Tx error: {0}")]
    TxError(String),
    #[error("Wallet error: {0}")]
    WalletError(String),
}
