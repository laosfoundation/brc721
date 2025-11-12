pub mod brc721_command;
pub mod register_collection;

use bitcoin::opcodes;
use bitcoin::script::{Builder, PushBytesBuf};
use bitcoin::{Amount, TxOut};
pub use brc721_command::Brc721Command;
use ethereum_types::H160;
pub use register_collection::{
    MessageDecodeError, RegisterCollectionMessage, RegisterCollectionTx,
};

pub type CollectionAddress = H160;
pub type Brc721Tx = [u8];
pub const BRC721_CODE: opcodes::Opcode = opcodes::all::OP_PUSHNUM_15;

pub fn brc721_output(payload: &[u8]) -> TxOut {
    let pb = PushBytesBuf::try_from(payload.to_vec()).unwrap();
    let script = Builder::new()
        .push_opcode(opcodes::all::OP_RETURN)
        .push_opcode(BRC721_CODE)
        .push_slice(pb)
        .into_script();
    TxOut {
        value: Amount::from_sat(0),
        script_pubkey: script,
    }
}
