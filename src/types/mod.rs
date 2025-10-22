pub mod brc721_command;
pub mod register_collection;

use bitcoin::opcodes;
pub use brc721_command::Brc721Command;
use bitcoin::script::{Builder, PushBytesBuf};
use bitcoin::ScriptBuf;
use bitcoin::{Amount, Transaction, TxOut};
use bitcoin::absolute::LockTime;
use bitcoin::transaction::Version;
use ethereum_types::H160;
pub use register_collection::{
    MessageDecodeError, RegisterCollectionMessage, RegisterCollectionTx,
};

pub type CollectionAddress = H160;
pub type Brc721Tx = [u8];
pub const BRC721_CODE: opcodes::Opcode = opcodes::all::OP_PUSHNUM_15;

pub fn brc721_op_return_script(payload: &[u8]) -> ScriptBuf {
    let pb = PushBytesBuf::try_from(payload.to_vec()).unwrap();
    Builder::new()
        .push_opcode(opcodes::all::OP_RETURN)
        .push_opcode(BRC721_CODE)
        .push_slice(pb)
        .into_script()
}

pub fn build_register_collection_tx(msg: &RegisterCollectionMessage) -> Transaction {
    let script = brc721_op_return_script(&msg.encode());
    Transaction {
        version: Version(2),
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![TxOut { value: Amount::from_sat(0), script_pubkey: script }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn op_return_script_matches_expected_hex() {
        let msg = RegisterCollectionMessage {
            collection_address: CollectionAddress::from_str(
                "ffff0123ffffffffffffffffffffffff3210ffff",
            )
            .unwrap(),
            rebaseable: false,
        };
        let payload = msg.encode();
        let script = brc721_op_return_script(&payload);
        let hex = hex::encode(script.as_bytes());
        assert_eq!(hex, "6a5f1600ffff0123ffffffffffffffffffffffff3210ffff00");
    }

    #[test]
    fn build_tx_has_single_zero_value_op_return_output() {
        let msg = RegisterCollectionMessage {
            collection_address: CollectionAddress::from_str(
                "ffff0123ffffffffffffffffffffffff3210ffff",
            )
            .unwrap(),
            rebaseable: false,
        };
        let tx = build_register_collection_tx(&msg);
        assert_eq!(tx.output.len(), 1);
        assert_eq!(tx.output[0].value, Amount::from_sat(0));
        let hex = hex::encode(tx.output[0].script_pubkey.as_bytes());
        assert_eq!(hex, "6a5f1600ffff0123ffffffffffffffffffffffff3210ffff00");
    }

    #[test]
    fn build_tx_rebase_true_has_flag_set_in_payload() {
        let msg = RegisterCollectionMessage {
            collection_address: CollectionAddress::from_str(
                "ffff0123ffffffffffffffffffffffff3210ffff",
            )
            .unwrap(),
            rebaseable: true,
        };
        let tx = build_register_collection_tx(&msg);
        let hex = hex::encode(tx.output[0].script_pubkey.as_bytes());
        assert!(hex.ends_with("01"));
    }
}
