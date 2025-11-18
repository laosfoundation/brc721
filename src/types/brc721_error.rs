use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum Brc721Error {
    #[error("script too short")]
    ScriptTooShort,
    #[error("wrong command: got {0}")]
    WrongCommand(u8),
    #[error("invalid rebase flag: {0}")]
    InvalidRebaseFlag(u8),
}

impl From<crate::types::MessageDecodeError> for Brc721Error {
    fn from(value: crate::types::MessageDecodeError) -> Self {
        match value {
            crate::types::MessageDecodeError::ScriptTooShort => Brc721Error::ScriptTooShort,
            crate::types::MessageDecodeError::WrongCommand(b) => Brc721Error::WrongCommand(b),
            crate::types::MessageDecodeError::InvalidRebaseFlag(b) => {
                Brc721Error::InvalidRebaseFlag(b)
            }
        }
    }
}
