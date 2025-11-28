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
    #[error("slot number {0} exceeds 96-bit range")]
    SlotNumberTooLarge(u128),
    #[error("slot range end must be greater than start (start: {start}, end: {end})")]
    InvalidSlotRange { start: u128, end: u128 },
    #[error("slot range tag {0} is invalid")]
    InvalidSlotRangeTag(u8),
    #[error("output index must be >= 1, got {0}")]
    InvalidOutputIndex(u64),
    #[error("slot range list cannot be empty for output {0}")]
    EmptySlotRangeList(u64),
    #[error("register ownership payload must include at least one slot mapping")]
    MissingSlotMappings,
    #[error("invalid rebase flag: {0}")]
    InvalidRebaseFlag(u8),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Tx error: {0}")]
    TxError(String),
    #[error("Wallet error: {0}")]
    WalletError(String),
}
