use super::{MixData, RegisterCollectionData, RegisterOwnershipData};
use crate::types::{Brc721Command, Brc721Error};
use bitcoin::Transaction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Brc721Payload {
    RegisterCollection(RegisterCollectionData),
    RegisterOwnership(RegisterOwnershipData),
    Mix(MixData),
}

impl Brc721Payload {
    pub fn command(&self) -> Brc721Command {
        match self {
            Brc721Payload::RegisterCollection(_) => Brc721Command::RegisterCollection,
            Brc721Payload::RegisterOwnership(_) => Brc721Command::RegisterOwnership,
            Brc721Payload::Mix(_) => Brc721Command::Mix,
        }
    }

    pub fn validate_in_tx(&self, bitcoin_tx: &Transaction) -> Result<(), Brc721Error> {
        match self {
            Brc721Payload::RegisterCollection(_) => Ok(()),
            Brc721Payload::RegisterOwnership(data) => data.validate_in_tx(bitcoin_tx),
            Brc721Payload::Mix(data) => data.validate_in_tx(bitcoin_tx),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(self.command().into());

        match self {
            Brc721Payload::RegisterCollection(data) => out.extend(data.to_bytes()),
            Brc721Payload::RegisterOwnership(data) => out.extend(data.to_bytes()),
            Brc721Payload::Mix(data) => out.extend(data.to_bytes()),
        };

        out
    }
}

impl TryFrom<&[u8]> for Brc721Payload {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let (&first, rest) = bytes.split_first().ok_or(Brc721Error::ScriptTooShort)?;

        let cmd = Brc721Command::try_from(first)?;

        let msg = match cmd {
            Brc721Command::RegisterCollection => {
                Brc721Payload::RegisterCollection(RegisterCollectionData::try_from(rest)?)
            }
            Brc721Command::RegisterOwnership => {
                Brc721Payload::RegisterOwnership(RegisterOwnershipData::try_from(rest)?)
            }
            Brc721Command::Mix => Brc721Payload::Mix(MixData::try_from(rest)?),
        };

        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::mix::IndexRange;
    use crate::types::register_ownership::{OwnershipGroup, SlotRange};
    use crate::types::{Brc721Command, Brc721Error, MixData};
    use crate::types::{RegisterCollectionData, RegisterOwnershipData};
    use ethereum_types::H160;

    #[test]
    fn command_matches_variant() {
        let addr = H160::from_low_u64_be(42);
        let data = RegisterCollectionData {
            evm_collection_address: addr,
            rebaseable: true,
        };
        let msg = Brc721Payload::RegisterCollection(data);

        assert_eq!(msg.command(), Brc721Command::RegisterCollection);
    }

    #[test]
    fn command_matches_register_ownership_variant() {
        let data = RegisterOwnershipData::new(
            840_000,
            2,
            vec![OwnershipGroup {
                ranges: vec![SlotRange { start: 0, end: 10 }],
            }],
        )
        .expect("valid ownership data");
        let msg = Brc721Payload::RegisterOwnership(data);

        assert_eq!(msg.command(), Brc721Command::RegisterOwnership);
    }

    #[test]
    fn command_matches_mix_variant() {
        let mix = MixData::new(vec![vec![IndexRange { start: 0, end: 2 }], Vec::new()], 1)
            .expect("valid mix data");
        let msg = Brc721Payload::Mix(mix);

        assert_eq!(msg.command(), Brc721Command::Mix);
    }

    #[test]
    fn to_bytes_and_from_bytes_roundtrip_register_collection() {
        let addr = H160::from_low_u64_be(42);
        let data = RegisterCollectionData {
            evm_collection_address: addr,
            rebaseable: true,
        };
        let msg = Brc721Payload::RegisterCollection(data.clone());

        let bytes = msg.to_bytes();

        // 1 command byte + RegisterCollectionData payload
        assert_eq!(bytes.len(), 1 + RegisterCollectionData::LEN);

        // the first byte must be the correct command
        let cmd_byte = bytes[0];
        let cmd = Brc721Command::try_from(cmd_byte).expect("valid command byte");
        assert_eq!(cmd, Brc721Command::RegisterCollection);

        // roundtrip
        let parsed = Brc721Payload::try_from(bytes.as_slice()).expect("parsing should succeed");
        assert_eq!(parsed, msg);
    }

    #[test]
    fn from_bytes_fails_on_empty_slice() {
        let bytes: [u8; 0] = [];

        let res = Brc721Payload::try_from(bytes.as_slice());

        match res {
            Err(Brc721Error::ScriptTooShort) => {}
            other => panic!("expected ScriptTooShort, got {:?}", other),
        }
    }

    #[test]
    fn from_bytes_fails_on_unknown_command() {
        // first byte = non-existent command, remaining bytes are random data
        let bytes = vec![0xFF, 0x00, 0x01];

        let res = Brc721Payload::try_from(bytes.as_slice());

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

        let res = Brc721Payload::try_from(bytes.as_slice());

        match res {
            Err(Brc721Error::InvalidLength(expected, actual)) => {
                assert_eq!(expected, RegisterCollectionData::LEN);
                assert_eq!(actual, 0);
            }
            other => panic!("expected InvalidLength(_, _), got {:?}", other),
        }
    }

    #[test]
    fn to_bytes_and_from_bytes_roundtrip_register_ownership() {
        let data = RegisterOwnershipData::new(
            123,
            1,
            vec![
                OwnershipGroup {
                    ranges: vec![
                        SlotRange { start: 0, end: 5 },
                        SlotRange { start: 10, end: 20 },
                    ],
                },
                OwnershipGroup {
                    ranges: vec![SlotRange {
                        start: 100,
                        end: 105,
                    }],
                },
            ],
        )
        .expect("valid ownership data");
        let msg = Brc721Payload::RegisterOwnership(data.clone());

        let bytes = msg.to_bytes();
        let parsed = Brc721Payload::try_from(bytes.as_slice()).expect("parsing should succeed");

        assert_eq!(parsed, msg);
    }

    #[test]
    fn to_bytes_and_from_bytes_roundtrip_mix() {
        let mix = MixData::new(
            vec![
                vec![IndexRange { start: 0, end: 3 }],
                Vec::new(),
                vec![IndexRange { start: 3, end: 5 }],
            ],
            1,
        )
        .expect("valid mix data");
        let msg = Brc721Payload::Mix(mix.clone());

        let bytes = msg.to_bytes();
        let parsed = Brc721Payload::try_from(bytes.as_slice()).expect("parsing should succeed");

        assert_eq!(parsed, msg);
    }

    #[test]
    fn from_bytes_rejects_register_ownership_with_zero_groups() {
        let bytes = vec![
            Brc721Command::RegisterOwnership.into(),
            // collection height (varint)
            1,
            // collection tx index (varint)
            2,
            // group count (varint)
            0,
        ];

        let res = Brc721Payload::try_from(bytes.as_slice());
        match res {
            Err(Brc721Error::InvalidGroupCount(0)) => {}
            other => panic!("expected InvalidGroupCount, got {:?}", other),
        }
    }
}
