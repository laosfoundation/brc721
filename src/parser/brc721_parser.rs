use crate::storage::traits::StorageWrite;
use crate::types::{Brc721Error, Brc721Message, Brc721Output};
use bitcoin::Block;

use crate::parser::BlockParser;

pub struct Brc721Parser;

impl Brc721Parser {
    pub fn new() -> Self {
        Self
    }

    fn digest<T: StorageWrite>(
        &self,
        tx: &T,
        output: &Brc721Output,
        block_height: u64,
        tx_index: u32,
    ) -> Result<(), Brc721Error> {
        match output.message() {
            Brc721Message::RegisterCollection(data) => {
                crate::parser::register_collection::digest(data, tx, block_height, tx_index)
            }
            Brc721Message::RegisterOwnership(data) => {
                crate::parser::register_ownership::digest(data, tx, block_height, tx_index)
            }
        }
    }
}

impl<T: StorageWrite> BlockParser<T> for Brc721Parser {
    fn parse_block(&self, tx: &T, block: &Block, block_height: u64) -> Result<(), Brc721Error> {
        let hash = block.block_hash();
        let hash_str = hash.to_string();

        for (tx_index, tx_data) in block.txdata.iter().enumerate() {
            let Some(first_output) = tx_data.output.first() else {
                continue;
            };
            let brc721_output = match Brc721Output::from_output(first_output) {
                Ok(output) => output,
                Err(e) => {
                    if e == Brc721Error::InvalidPayload {
                        log::debug!("Skipping output: {:?}", e);
                    } else {
                        log::warn!(
                            "Invalid BRC-721 message at block {} tx {}: {:?}",
                            block_height,
                            tx_index,
                            e
                        );
                    }
                    continue;
                }
            };

            log::info!(
                "📦 Found BRC-721 tx at block {}, tx {}",
                block_height,
                tx_index
            );

            if let Err(ref e) = self.digest(tx, &brc721_output, block_height, tx_index as u32) {
                log::warn!("{:?}", e);
            }
        }
        // Persist last processed block once per block
        if let Err(e) = tx.save_last(block_height, &hash_str) {
            log::error!(
                "storage error saving block {} at height {}: {}",
                hash,
                block_height,
                e
            );
            return Err(Brc721Error::StorageError(e.to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::traits::{
        Block as StorageBlock, Collection, CollectionKey, StorageRead, StorageTx, StorageWrite,
    };
    use crate::storage::Storage;
    use crate::types::Brc721Command;
    use crate::types::BRC721_CODE;
    use anyhow::anyhow;
    use bitcoin::blockdata::constants::genesis_block;
    use bitcoin::hashes::Hash;
    use bitcoin::opcodes::all::OP_RETURN;
    use bitcoin::Network;
    use bitcoin::{Amount, Block, OutPoint, ScriptBuf, Transaction, TxIn, TxOut};
    use ethereum_types::H160;
    use hex::FromHex;
    use std::sync::{Arc, Mutex};

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
        let temp_dir = tempfile::tempdir().expect("create temp dir for db");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_parser_test.db"));
        storage.init().expect("init the database");
        let parser = Brc721Parser::new();
        let tx = storage.begin_tx().expect("init the tx");
        let r = parser.parse_block(&tx, &block, 0);
        assert!(r.is_ok());
        tx.commit().unwrap();
    }

    struct DummyStorageInner {
        last_height: Mutex<Option<u64>>,
        last_hash: Mutex<Option<String>>,
        fail: bool,
    }

    #[derive(Clone)]
    struct DummyStorage {
        inner: Arc<DummyStorageInner>,
    }

    impl DummyStorage {
        fn new(fail: bool) -> Self {
            Self {
                inner: Arc::new(DummyStorageInner {
                    last_height: Mutex::new(None),
                    last_hash: Mutex::new(None),
                    fail,
                }),
            }
        }
    }

    impl StorageTx for DummyStorage {
        fn commit(self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl Storage for DummyStorage {
        type Tx = DummyStorage;

        fn begin_tx(&self) -> anyhow::Result<Self::Tx> {
            Ok(self.clone())
        }
    }

    impl StorageRead for DummyStorage {
        fn load_last(&self) -> anyhow::Result<Option<StorageBlock>> {
            Ok(self
                .inner
                .last_height
                .lock()
                .unwrap()
                .map(|h| StorageBlock {
                    height: h,
                    hash: String::new(),
                }))
        }

        fn load_collection(&self, _id: &CollectionKey) -> anyhow::Result<Option<Collection>> {
            Ok(None)
        }

        fn list_collections(&self) -> anyhow::Result<Vec<Collection>> {
            Ok(Vec::new())
        }
    }

    impl StorageWrite for DummyStorage {
        fn save_last(&self, height: u64, hash: &str) -> anyhow::Result<()> {
            if self.inner.fail {
                return Err(anyhow!("fail"));
            }
            *self.inner.last_height.lock().unwrap() = Some(height);
            *self.inner.last_hash.lock().unwrap() = Some(hash.to_string());
            Ok(())
        }

        fn save_collection(
            &self,
            _key: CollectionKey,
            _evm_collection_address: H160,
            _rebaseable: bool,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn make_parser_with_storage(fail_storage: bool) -> (DummyStorage, Brc721Parser) {
        let storage = DummyStorage::new(fail_storage);
        let parser = Brc721Parser::new();
        (storage, parser)
    }

    #[test]
    fn parse_block_success_saves_height_and_hash() {
        let (storage, parser) = make_parser_with_storage(false);

        let block = genesis_block(Network::Regtest);
        let height = 42;
        let tx = storage.clone();
        parser.parse_block(&tx, &block, height).unwrap();

        assert_eq!(*storage.inner.last_height.lock().unwrap(), Some(height));
        assert_eq!(
            *storage.inner.last_hash.lock().unwrap(),
            Some(block.block_hash().to_string())
        );
    }

    #[test]
    fn parse_block_storage_error_returns_error_and_does_not_persist() {
        let (storage, parser) = make_parser_with_storage(true);

        let block = genesis_block(Network::Regtest);
        let height = 1;
        let tx = storage.clone();
        assert!(parser.parse_block(&tx, &block, height).is_err());

        assert_eq!(*storage.inner.last_height.lock().unwrap(), None);
        assert_eq!(*storage.inner.last_hash.lock().unwrap(), None);
    }
}
