mod brc721_command;
mod brc721_error;
mod brc721_output;
mod register_collection;

use bitcoin::opcodes;
use bitcoin::TxOut;
pub use brc721_command::Brc721Command;
pub use brc721_error::Brc721Error;
pub use brc721_output::Brc721Output;
pub use register_collection::{
    MessageDecodeError, RegisterCollectionMessage, RegisterCollectionTx,
};

pub const BRC721_CODE: opcodes::Opcode = opcodes::all::OP_PUSHNUM_15;

pub fn build_brc721_output(payload: &[u8]) -> TxOut {
    Brc721Output::from_slice(payload).into_txout()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::Amount;

    #[test]
    fn test_brc721_output_creates_correct_txout() {
        let payload = [0xaa, 0xbb, 0xcc];
        let txout = build_brc721_output(&payload);
        // Value should be zero
        assert_eq!(txout.value, Amount::from_sat(0));
        assert_eq!(
            txout.script_pubkey.to_string(),
            "OP_RETURN OP_PUSHNUM_15 OP_PUSHBYTES_3 aabbcc"
        );
    }
}
