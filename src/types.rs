use bitcoin::opcodes;
use ethereum_types::H160;
use std::convert::TryFrom;

pub type CollectionAddress = H160;
pub type Brc721Tx = [u8];

pub const BRC721_CODE: opcodes::Opcode = opcodes::all::OP_PUSHNUM_15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterCollectionPayload {
    /// The 160-bit EVM address of the collection's smart contract.
    pub collection_address: CollectionAddress,

    /// A boolean indicating whether the collection supports future Rebase transactions.
    pub rebaseable: bool,
}

/// Enum representing BRC-721 commands, using `u8` as discriminants.
#[repr(u8)]
pub enum Brc721Command {
    RegisterCollection = 0x00,
}

impl TryFrom<u8> for Brc721Command {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Brc721Command::RegisterCollection),
            _ => Err(()),
        }
    }
}
