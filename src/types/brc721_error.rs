use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum Brc721Error {
    #[error("script too short")]
    ScriptTooShort,
    #[error("unknown command: got {0}")]
    UnknownCommand(u8),
    #[error("invalid rebase flag: {0}")]
    InvalidRebaseFlag(u8),
}
