use crate::types::{Brc721Command, Brc721Error};

use super::RegisterCollectionData;

// pub type Brc721Message = Vec<u8>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Brc721Message {
    RegisterCollection(RegisterCollectionData),
}

impl Brc721Message {
    pub fn command(&self) -> Brc721Command {
        match self {
            Brc721Message::RegisterCollection(_) => Brc721Command::RegisterCollection,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Brc721Error> {
        let (&first, rest) = bytes.split_first().ok_or(Brc721Error::ScriptTooShort)?;

        let cmd = Brc721Command::try_from(first)?;

        let msg = match cmd {
            Brc721Command::RegisterCollection => {
                Brc721Message::RegisterCollection(RegisterCollectionData::from_bytes(rest)?)
            }
        };

        Ok(msg)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(self.command().into());

        match self {
            Brc721Message::RegisterCollection(data) => out.extend(data.to_bytes()),
        };

        out
    }
}
