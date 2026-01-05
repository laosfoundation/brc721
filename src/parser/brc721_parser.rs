use crate::storage::traits::{StorageRead, StorageWrite};
use crate::types::{parse_brc721_tx, Brc721Error, Brc721Payload, Brc721Tx};
use bitcoin::Block;
use bitcoin::Transaction;

use crate::parser::BlockParser;

pub struct Brc721Parser;

impl Brc721Parser {
    pub fn new() -> Self {
        Self
    }

    fn digest_spends<T: StorageWrite>(
        &self,
        storage: &T,
        bitcoin_tx: &Transaction,
        block_height: u64,
        spent_txid: bitcoin::Txid,
    ) -> Result<(), Brc721Error> {
        for txin in &bitcoin_tx.input {
            let prevout = txin.previous_output;
            if prevout.is_null() {
                continue;
            }
            storage
                .mark_ownership_outpoint_spent(prevout, block_height, spent_txid)
                .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
        }
        Ok(())
    }

    fn parse_tx<T: StorageRead + StorageWrite>(
        &self,
        storage: &T,
        bitcoin_tx: &Transaction,
        block_height: u64,
        tx_index: u32,
    ) -> Result<(), Brc721Error> {
        let txid = bitcoin_tx.compute_txid();
        self.digest_spends(storage, bitcoin_tx, block_height, txid)?;
        let brc721_tx: Brc721Tx<'_> = match parse_brc721_tx(bitcoin_tx) {
            Ok(Some(tx)) => tx,
            Ok(None) => return Ok(()),
            Err(e) => {
                log::warn!(
                    "Invalid BRC-721 message at block {} tx {}: {:?}",
                    block_height,
                    tx_index,
                    e
                );
                return Ok(());
            }
        };

        log::info!(
            "📦 Found BRC-721 tx at block {}, tx {} (txid={}, cmd={:?})",
            block_height,
            tx_index,
            txid,
            brc721_tx.payload().command()
        );

        self.digest_brc721_tx(storage, &brc721_tx, block_height, tx_index)?;
        Ok(())
    }

    fn digest_brc721_tx<T: StorageRead + StorageWrite>(
        &self,
        storage: &T,
        brc721_tx: &Brc721Tx<'_>,
        block_height: u64,
        tx_index: u32,
    ) -> Result<(), Brc721Error> {
        brc721_tx.validate()?;

        match brc721_tx.payload() {
            Brc721Payload::RegisterCollection(payload) => {
                crate::parser::register_collection::digest(
                    payload,
                    brc721_tx,
                    storage,
                    block_height,
                    tx_index,
                )
            }
            Brc721Payload::RegisterOwnership(payload) => crate::parser::register_ownership::digest(
                payload,
                brc721_tx,
                storage,
                block_height,
                tx_index,
            ),
        }
    }
}

impl<T: StorageRead + StorageWrite> BlockParser<T> for Brc721Parser {
    fn parse_block(
        &self,
        storage: &T,
        block: &Block,
        block_height: u64,
    ) -> Result<(), Brc721Error> {
        let hash = block.block_hash();
        let hash_str = hash.to_string();

        for (tx_index, bitcoin_tx) in block.txdata.iter().enumerate() {
            self.parse_tx(storage, bitcoin_tx, block_height, tx_index as u32)?;
        }
        // Persist last processed block once per block
        if let Err(e) = storage.save_last(block_height, &hash_str) {
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
        Block as StorageBlock, Collection, CollectionKey, OwnershipRange, StorageRead, StorageTx,
        StorageWrite,
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

    #[test]
    fn parse_block_register_ownership_persists_and_spend_removes_it() {
        use crate::types::RegisterCollectionData;
        use crate::types::{Brc721OpReturnOutput, Brc721Payload, RegisterOwnershipData, SlotRanges};
        use bitcoin::hashes::hash160;
        use bitcoin::{
            absolute, transaction, Address, Network, OutPoint, ScriptBuf, Sequence, TxIn, TxOut,
        };
        use ethereum_types::H160 as EthH160;
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("create temp dir for db");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_ownership_test.db"));
        storage.init().expect("init the database");
        let parser = Brc721Parser::new();

        let owner_address = Address::from_str(
            "bcrt1p8wpt9v4frpf3tkn0srd97pksgsxc5hs52lafxwru9kgeephvs7rqjeprhg",
        )
        .unwrap()
        .require_network(Network::Regtest)
        .unwrap();
        let owner_script = owner_address.script_pubkey();
        let owner_hash = hash160::Hash::hash(owner_script.as_bytes());
        let owner_h160 = EthH160::from_slice(owner_hash.as_byte_array());

        let collection_payload = Brc721Payload::RegisterCollection(RegisterCollectionData {
            evm_collection_address: EthH160::from_low_u64_be(1),
            rebaseable: false,
        });
        let collection_op_return =
            Brc721OpReturnOutput::new(collection_payload).into_txout().unwrap();
        let collection_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![],
            output: vec![collection_op_return],
        };

        let slots = SlotRanges::from_str("0..=9,42").expect("slots parse");
        let ownership = RegisterOwnershipData::for_single_output(1, 0, slots).unwrap();
        let ownership_payload = Brc721Payload::RegisterOwnership(ownership);
        let ownership_op_return =
            Brc721OpReturnOutput::new(ownership_payload).into_txout().unwrap();

        let ownership_output = TxOut {
            value: Amount::from_sat(546),
            script_pubkey: owner_script.clone(),
        };

        let ownership_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![],
            output: vec![ownership_op_return, ownership_output],
        };
        let ownership_txid = ownership_tx.compute_txid();

        let mut block1 = genesis_block(Network::Regtest);
        block1.txdata = vec![collection_tx, ownership_tx];

        let tx = storage.begin_tx().unwrap();
        parser.parse_block(&tx, &block1, 1).unwrap();

        let owned = tx.list_unspent_ownership_by_owner(owner_h160).unwrap();
        assert_eq!(owned.len(), 2);

        assert!(tx
            .has_unspent_slot_overlap(&CollectionKey::new(1, 0), 0, 9)
            .unwrap());

        let spend_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: ownership_txid,
                    vout: 1,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: bitcoin::Witness::default(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(0),
                script_pubkey: ScriptBuf::new(),
            }],
        };

        let mut block2 = genesis_block(Network::Regtest);
        block2.txdata = vec![spend_tx];
        parser.parse_block(&tx, &block2, 2).unwrap();

        let owned_after = tx.list_unspent_ownership_by_owner(owner_h160).unwrap();
        assert_eq!(owned_after.len(), 0);

        assert!(!tx
            .has_unspent_slot_overlap(&CollectionKey::new(1, 0), 0, 9)
            .unwrap());

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

        fn has_unspent_slot_overlap(
            &self,
            _collection_id: &CollectionKey,
            _slot_start: u128,
            _slot_end: u128,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }

        fn list_unspent_ownership_by_owner(
            &self,
            _owner_h160: H160,
        ) -> anyhow::Result<Vec<OwnershipRange>> {
            Ok(Vec::new())
        }

        fn list_unspent_ownership_by_owners(
            &self,
            _owner_h160s: &[H160],
        ) -> anyhow::Result<Vec<OwnershipRange>> {
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

        fn insert_ownership_range(
            &self,
            _collection_id: CollectionKey,
            _owner_h160: H160,
            _outpoint: OutPoint,
            _slot_start: u128,
            _slot_end: u128,
            _created_height: u64,
            _created_tx_index: u32,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn mark_ownership_outpoint_spent(
            &self,
            _outpoint: OutPoint,
            _spent_height: u64,
            _spent_txid: bitcoin::Txid,
        ) -> anyhow::Result<usize> {
            Ok(0)
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

    #[test]
    fn parse_block_ignores_register_ownership_for_unknown_collection() {
        use crate::types::{Brc721OpReturnOutput, RegisterOwnershipData, SlotRanges};
        use std::str::FromStr;

        let (storage, parser) = make_parser_with_storage(false);

        let slots = SlotRanges::from_str("0").expect("slots parse");
        let ownership =
            RegisterOwnershipData::for_single_output(0, 0, slots).expect("ownership payload");

        let op_return = Brc721OpReturnOutput::new(Brc721Payload::RegisterOwnership(ownership))
            .into_txout()
            .expect("opreturn txout");

        let tx = Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![],
            output: vec![
                op_return,
                TxOut {
                    value: Amount::from_sat(0),
                    script_pubkey: ScriptBuf::new(),
                },
            ],
        };

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![tx];

        let height = 7;
        let tx = storage.clone();
        assert!(parser.parse_block(&tx, &block, height).is_ok());
    }
}
