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
    #[error("invalid ownership payload: {0}")]
    InvalidOwnershipPayload(&'static str),
    #[error("invalid slot value: {0}")]
    InvalidSlotValue(u128),
    #[error("invalid slot range: start {start} > end {end}")]
    InvalidSlotRange { start: u128, end: u128 },
    #[error("ownership registration requires at least one input")]
    MissingOwnershipInput,
    #[error("unable to derive hash160 from input0 script or witness")]
    OwnershipProofUnavailable,
    #[error("collection {0} not found")]
    CollectionNotFound(String),
    #[error("token already registered for collection {collection} slot {slot} owner {owner}")]
    TokenAlreadyRegistered {
        collection: String,
        slot: u128,
        owner: String,
    },
    #[error("ownership output index {requested} missing (tx has {available} outputs)")]
    OwnershipOutputMissing { requested: usize, available: usize },
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Tx error: {0}")]
    TxError(String),
    #[error("Wallet error: {0}")]
    WalletError(String),
}
