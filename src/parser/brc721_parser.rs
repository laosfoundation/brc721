use crate::types::{Brc721Error, Brc721Message, Brc721Output};
use bitcoin::Block;

use crate::parser::BlockParse;

pub struct Brc721Parser {
    storage: std::sync::Arc<dyn crate::storage::Storage + Send + Sync>,
}

impl Brc721Parser {
    pub fn new(storage: std::sync::Arc<dyn crate::storage::Storage + Send + Sync>) -> Self {
        Self { storage }
    }

    fn digest(
        &self,
        output: &Brc721Output,
        block_height: u64,
        tx_index: u32,
    ) -> Result<(), Brc721Error> {
        match output.message() {
            Brc721Message::RegisterCollection(data) => crate::parser::register_collection::digest(
                data,
                self.storage.as_ref(),
                block_height,
                tx_index,
            ),
        }
    }
}

impl BlockParse for Brc721Parser {
    fn parse_block(&self, block: &Block, block_height: u64) -> Result<(), Brc721Error> {
        for (tx_index, tx) in block.txdata.iter().enumerate() {
            let Some(first_output) = tx.output.first() else {
                continue;
            };
            let brc721_output = match Brc721Output::from_output(first_output) {
                Ok(output) => output,
                Err(e) => {
                    log::debug!("Skipping output: {:?}", e);
                    continue;
                }
            };

            log::info!(
                "📦 Found BRC-721 tx at block {}, tx {}",
                block_height,
                tx_index
            );

            if let Err(ref e) = self.digest(&brc721_output, block_height, tx_index as u32) {
                log::warn!("{:?}", e);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Brc721Command;
    use crate::types::BRC721_CODE;
    use bitcoin::hashes::Hash;
    use bitcoin::opcodes::all::OP_RETURN;
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
            .push_opcode(OP_RETURN)
            .push_opcode(BRC721_CODE)
            .push_slice(bitcoin::script::PushBytesBuf::try_from(payload.to_vec()).unwrap())
            .into_script()
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
        let parser = Brc721Parser::new(std::sync::Arc::new(storage));
        let r = parser.parse_block(&block, 0);
        assert!(r.is_ok());
    }
}
