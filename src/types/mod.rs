mod brc721_command;
mod brc721_error;
mod brc721_message;
mod brc721_output;
mod register_collection;
mod register_ownership;

use bitcoin::opcodes;
pub use brc721_command::Brc721Command;
pub use brc721_error::Brc721Error;
pub use brc721_message::Brc721Message;
pub use brc721_output::Brc721Output;
pub use register_collection::RegisterCollectionData;
#[allow(unused_imports)]
pub use register_ownership::{
    BitcoinCollectionId, RegisterOwnershipData, SlotMapping, SlotNumber, SlotRange, TokenId,
};

pub const BRC721_CODE: opcodes::Opcode = opcodes::all::OP_PUSHNUM_15;
