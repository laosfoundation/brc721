use crate::types::{Brc721Command, Brc721Output};
use bitcoin::blockdata::opcodes::all as opcodes;
use bitcoin::blockdata::script::Instruction;
use bitcoin::Block;
use bitcoin::Transaction;
use bitcoin::TxOut;

mod register_collection;

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum Brc721Error {
    #[error("script too short")]
    ScriptTooShort,
    #[error("wrong command: got {0}")]
    WrongCommand(u8),
    #[error("invalid rebase flag: {0}")]
    InvalidRebaseFlag(u8),
}

impl From<crate::types::MessageDecodeError> for Brc721Error {
    fn from(value: crate::types::MessageDecodeError) -> Self {
        match value {
            crate::types::MessageDecodeError::ScriptTooShort => Brc721Error::ScriptTooShort,
            crate::types::MessageDecodeError::WrongCommand(b) => Brc721Error::WrongCommand(b),
            crate::types::MessageDecodeError::InvalidRebaseFlag(b) => {
                Brc721Error::InvalidRebaseFlag(b)
            }
        }
    }
}

pub struct Parser {
    storage: std::sync::Arc<dyn crate::storage::Storage + Send + Sync>,
}

impl Parser {
    pub fn new(storage: std::sync::Arc<dyn crate::storage::Storage + Send + Sync>) -> Self {
        Self { storage }
    }

    pub fn parse_block(&self, block: &Block, block_height: u64) -> Result<(), Brc721Error> {
        for (tx_index, tx) in block.txdata.iter().enumerate() {
            let output = match get_first_output_if_op_return(tx) {
                Some(output) => output,
                None => continue,
            };

            let brc721_output = match get_brc721_output(output) {
                Some(output) => output,
                None => continue,
            };

            log::info!(
                "📦 Found BRC-721 tx at block {}, tx {}",
                block_height,
                tx_index
            );

            if let Some(Err(ref e)) = self.digest(&brc721_output, block_height, tx_index as u32) {
                log::warn!("{:?}", e);
            }
        }
        Ok(())
    }

    fn digest(
        &self,
        output: &Brc721Output,
        block_height: u64,
        tx_index: u32,
    ) -> Option<Result<(), Brc721Error>> {
        let command = match output.command() {
            Some(cmd) => cmd,
            None => return Some(Err(Brc721Error::ScriptTooShort)),
        };
        let payload = match output.payload() {
            Some(payload) => payload,
            None => return Some(Err(Brc721Error::ScriptTooShort)),
        };

        let result = match command {
            Brc721Command::RegisterCollection => register_collection::digest(
                payload.as_slice(),
                self.storage.clone(),
                block_height,
                tx_index,
            ),
        };
        Some(result)
    }
}

fn get_brc721_output(output: &TxOut) -> Option<Brc721Output> {
    Brc721Output::from_output(output)
}

fn get_first_output_if_op_return(tx: &Transaction) -> Option<&TxOut> {
    let out0 = tx.output.first()?;
    let mut it = out0.script_pubkey.instructions();
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_RETURN)) => Some(out0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BRC721_CODE;
    use bitcoin::hashes::Hash;
    use bitcoin::{Amount, Block, OutPoint, ScriptBuf, Transaction, TxIn, TxOut};
    use hex::FromHex;

    fn build_payload(addr20: [u8; 20], rebase: u8) -> Vec<u8> {
        let mut v = Vec::with_capacity(1 + 20 + 1);
        v.push(Brc721Command::RegisterCollection as u8);
        v.extend_from_slice(&addr20);
        v.push(rebase);
        v
    }

    fn script_for_payload(payload: &[u8]) -> ScriptBuf {
        use bitcoin::script::Builder;
        Builder::new()
            .push_opcode(opcodes::OP_RETURN)
            .push_opcode(BRC721_CODE)
            .push_slice(bitcoin::script::PushBytesBuf::try_from(payload.to_vec()).unwrap())
            .into_script()
    }

    #[test]
    fn test_get_brc721_tx_extracts_payload() {
        let addr = [0x11u8; 20];
        let payload = build_payload(addr, 1);
        let script = script_for_payload(&payload);
        let tx = Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: bitcoin::Sequence(0xffffffff),
                witness: bitcoin::Witness::default(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(0),
                script_pubkey: script,
            }],
        };
        let out0 = get_first_output_if_op_return(&tx).expect("must be op_return");
        let extracted = get_brc721_output(out0).expect("must extract payload");
        assert_eq!(extracted.payload().unwrap(), payload);
    }

    #[test]
    fn test_script_hex_starts_with_6a5f16_and_matches_expected() {
        let addr = <[u8; 20]>::from_hex("ffff0123ffffffffffffffffffffffff3210ffff").unwrap();
        let payload = build_payload(addr, 0x00);
        let script = script_for_payload(&payload);
        let hex = hex::encode(script.as_bytes());
        assert_eq!(hex, "6a5f1600ffff0123ffffffffffffffffffffffff3210ffff00");
    }

    #[test]
    fn test_full_parse_flow_register_collection() {
        let addr = [0xABu8; 20];
        let payload = build_payload(addr, 0);
        let script = script_for_payload(&payload);
        let tx = Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: bitcoin::Sequence(0xffffffff),
                witness: bitcoin::Witness::default(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(0),
                script_pubkey: script,
            }],
        };
        let header = bitcoin::block::Header {
            version: bitcoin::block::Version::ONE,
            prev_blockhash: bitcoin::BlockHash::from_raw_hash(
                bitcoin::hashes::sha256d::Hash::all_zeros(),
            ),
            merkle_root: bitcoin::TxMerkleNode::from_raw_hash(
                bitcoin::hashes::sha256d::Hash::all_zeros(),
            ),
            time: 0,
            bits: bitcoin::CompactTarget::from_consensus(0),
            nonce: 0,
        };
        let block = Block {
            header,
            txdata: vec![tx],
        };
        let storage =
            crate::storage::SqliteStorage::new(std::env::temp_dir().join("test_db.sqlite"));
        let parser = Parser {
            storage: std::sync::Arc::new(storage),
        };
        let r = parser.parse_block(&block, 0);
        assert!(r.is_ok());
    }
}
