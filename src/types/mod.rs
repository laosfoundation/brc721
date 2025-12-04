mod brc721_command;
mod brc721_error;
mod brc721_message;
mod brc721_output;
mod brc721_token;
mod register_collection;

#[allow(unused_imports)]
pub use self::brc721_token::Brc721Token;
use bitcoin::opcodes;
pub use brc721_command::Brc721Command;
pub use brc721_error::Brc721Error;
pub use brc721_message::Brc721Message;
pub use brc721_output::Brc721Output;
pub use register_collection::RegisterCollectionData;

pub const BRC721_CODE: opcodes::Opcode = opcodes::all::OP_PUSHNUM_15;
