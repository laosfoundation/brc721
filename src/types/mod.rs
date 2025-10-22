pub mod brc721_command;
pub mod register_collection;

use bitcoin::opcodes;
pub use brc721_command::Brc721Command;
use ethereum_types::H160;
pub use register_collection::{
    MessageDecodeError, RegisterCollectionMessage, RegisterCollectionTx,
};

pub type CollectionAddress = H160;
pub type Brc721Tx = [u8];
pub const BRC721_CODE: opcodes::Opcode = opcodes::all::OP_PUSHNUM_15;
