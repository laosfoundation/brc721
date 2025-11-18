use super::RegisterCollectionData;
use crate::types::{Brc721Command, Brc721Error};

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

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(self.command().into());

        match self {
            Brc721Message::RegisterCollection(data) => out.extend(data.to_bytes()),
        };

        out
    }
}

impl TryFrom<&[u8]> for Brc721Message {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let (&first, rest) = bytes.split_first().ok_or(Brc721Error::ScriptTooShort)?;

        let cmd = Brc721Command::try_from(first)?;

        let msg = match cmd {
            Brc721Command::RegisterCollection => {
                Brc721Message::RegisterCollection(RegisterCollectionData::try_from(rest)?)
            }
        };

        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RegisterCollectionData;
    use crate::types::{Brc721Command, Brc721Error};
    use ethereum_types::H160;

    #[test]
    fn command_matches_variant() {
        let addr = H160::from_low_u64_be(42);
        let data = RegisterCollectionData {
            evm_collection_address: addr,
            rebaseable: true,
        };
        let msg = Brc721Message::RegisterCollection(data);

        assert_eq!(msg.command(), Brc721Command::RegisterCollection);
    }

    #[test]
    fn to_bytes_and_from_bytes_roundtrip_register_collection() {
        let addr = H160::from_low_u64_be(42);
        let data = RegisterCollectionData {
            evm_collection_address: addr,
            rebaseable: true,
        };
        let msg = Brc721Message::RegisterCollection(data.clone());

        let bytes = msg.to_bytes();

        // 1 command byte + RegisterCollectionData payload
        assert_eq!(bytes.len(), 1 + RegisterCollectionData::LEN);

        // the first byte must be the correct command
        let cmd_byte = bytes[0];
        let cmd = Brc721Command::try_from(cmd_byte).expect("valid command byte");
        assert_eq!(cmd, Brc721Command::RegisterCollection);

        // roundtrip
        let parsed = Brc721Message::try_from(bytes.as_slice()).expect("parsing should succeed");
        assert_eq!(parsed, msg);
    }

    #[test]
    fn from_bytes_fails_on_empty_slice() {
        let bytes: [u8; 0] = [];

        let res = Brc721Message::try_from(bytes.as_slice());

        match res {
            Err(Brc721Error::ScriptTooShort) => {}
            other => panic!("expected ScriptTooShort, got {:?}", other),
        }
    }

    #[test]
    fn from_bytes_fails_on_unknown_command() {
        // first byte = non-existent command, remaining bytes are random data
        let bytes = vec![0xFF, 0x00, 0x01];

        let res = Brc721Message::try_from(bytes.as_slice());

        match res {
            Err(Brc721Error::UnknownCommand(0xFF)) => {}
            other => panic!("expected UnknownCommand(0xFF), got {:?}", other),
        }
    }

    #[test]
    fn from_bytes_propagates_invalid_payload_error() {
        // valid command but payload too short for RegisterCollectionData
        let bytes = vec![Brc721Command::RegisterCollection.into()];
        // no payload, so `rest` will be empty

        let res = Brc721Message::try_from(bytes.as_slice());

        match res {
            Err(Brc721Error::InvalidLength(expected, actual)) => {
                assert_eq!(expected, RegisterCollectionData::LEN);
                assert_eq!(actual, 0);
            }
            other => panic!("expected InvalidLength(_, _), got {:?}", other),
        }
    }
}
