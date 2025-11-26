use thiserror::Error;

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
    #[error("invalid rebase flag: {0}")]
    InvalidRebaseFlag(u8),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Tx error: {0}")]
    TxError(String),
    #[error("Wallet error: {0}")]
    WalletError(String),
}
