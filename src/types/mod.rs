mod brc721_command;
mod brc721_error;
mod brc721_op_return_output;
mod brc721_payload;
mod brc721_token;
mod brc721_tx;
pub(crate) mod mix;
mod register_collection;
mod register_ownership;
pub(crate) mod varint96;

pub use self::brc721_token::Brc721Token;
use bitcoin::opcodes;
pub use brc721_command::Brc721Command;
pub use brc721_error::Brc721Error;
pub use brc721_op_return_output::Brc721OpReturnOutput;
pub use brc721_payload::Brc721Payload;
pub use brc721_tx::{parse_brc721_tx, Brc721Tx};
pub use mix::{IndexRanges, MixData};
pub use register_collection::RegisterCollectionData;
pub use register_ownership::{RegisterOwnershipData, SlotRanges};

pub const BRC721_CODE: opcodes::Opcode = opcodes::all::OP_PUSHNUM_15;

pub(crate) fn h160_from_script_pubkey(script_pubkey: &bitcoin::ScriptBuf) -> ethereum_types::H160 {
    use bitcoin::hashes::{hash160, Hash};

    let hash = hash160::Hash::hash(script_pubkey.as_bytes());
    ethereum_types::H160::from_slice(hash.as_byte_array())
}
