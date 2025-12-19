use crate::scanner::BitcoinRpc;
use crate::storage::traits::{CollectionKey, OwnershipRange, StorageRead, StorageWrite};
use crate::types::{Brc721Error, Brc721Tx, RegisterOwnershipData};
use bitcoin::hashes::{hash160, Hash as _};
use bitcoin::OutPoint;
use ethereum_types::H160;

pub fn digest<C: BitcoinRpc, S: StorageRead + StorageWrite>(
    rpc: &C,
    payload: &RegisterOwnershipData,
    brc721_tx: &Brc721Tx<'_>,
    storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let bitcoin_tx = brc721_tx.bitcoin_tx();

    let input0 = bitcoin_tx.input.first().ok_or_else(|| {
        Brc721Error::TxError(format!(
            "register-ownership requires at least 1 input (block {} tx {})",
            block_height, tx_index
        ))
    })?;
    if input0.previous_output.is_null() {
        return Err(Brc721Error::TxError(format!(
            "register-ownership input0 cannot be coinbase (block {} tx {})",
            block_height, tx_index
        )));
    }

    let collection_key = CollectionKey::new(payload.collection_height, payload.collection_tx_index);
    let collection = storage
        .load_collection(&collection_key)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
    if collection.is_none() {
        return Err(Brc721Error::TxError(format!(
            "register-ownership references unknown collection {} (block {} tx {})",
            collection_key, block_height, tx_index
        )));
    }

    let prev_tx = rpc
        .get_raw_transaction(&input0.previous_output.txid)
        .map_err(|e| Brc721Error::RpcError(e.to_string()))?;
    let prev_vout = input0.previous_output.vout as usize;
    let prev_txout = prev_tx.output.get(prev_vout).ok_or_else(|| {
        Brc721Error::TxError(format!(
            "register-ownership input0 prevout vout {} out of bounds (prev outputs={}) (block {} tx {})",
            prev_vout,
            prev_tx.output.len(),
            block_height,
            tx_index
        ))
    })?;

    let initial_owner_h160 = script_h160(&prev_txout.script_pubkey);

    // Reject any duplicate slot assignments within the same tx (regardless of output grouping).
    let mut ranges: Vec<(u128, u128)> = payload
        .groups
        .iter()
        .flat_map(|g| g.ranges.iter().map(|r| (r.start, r.end)))
        .collect();
    ranges.sort_by_key(|(start, _end)| *start);
    for window in ranges.windows(2) {
        let (prev_start, prev_end) = window[0];
        let (next_start, next_end) = window[1];
        if next_start <= prev_end {
            return Err(Brc721Error::TxError(format!(
                "register-ownership has overlapping slot ranges {}-{} and {}-{} (block {} tx {})",
                prev_start, prev_end, next_start, next_end, block_height, tx_index
            )));
        }
    }

    // Reject registrations that overlap with already-registered tokens.
    for (slot_start, slot_end) in &ranges {
        let overlaps = storage
            .has_ownership_overlap(&collection_key, initial_owner_h160, *slot_start, *slot_end)
            .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
        if overlaps {
            return Err(Brc721Error::TxError(format!(
                "register-ownership attempts to re-register tokens in {}:{}-{} (block {} tx {})",
                collection_key, slot_start, slot_end, block_height, tx_index
            )));
        }
    }

    let txid = bitcoin_tx.compute_txid();
    let mut out = Vec::new();
    for group in &payload.groups {
        let vout = group.output_index as usize;
        let tx_out = bitcoin_tx.output.get(vout).ok_or_else(|| {
            Brc721Error::TxError(format!(
                "register-ownership output_index {} out of bounds (tx outputs={}) (block {} tx {})",
                group.output_index,
                bitcoin_tx.output.len(),
                block_height,
                tx_index
            ))
        })?;

        let owner_h160 = script_h160(&tx_out.script_pubkey);
        let outpoint = OutPoint {
            txid,
            vout: group.output_index as u32,
        };

        for range in &group.ranges {
            out.push(OwnershipRange {
                collection: collection_key.clone(),
                initial_owner_h160,
                slot_start: range.start,
                slot_end: range.end,
                owner_h160,
                outpoint,
            });
        }
    }

    storage
        .save_ownership_ranges(&out)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?;

    Ok(())
}

fn script_h160(script_pubkey: &bitcoin::ScriptBuf) -> H160 {
    let h = hash160::Hash::hash(script_pubkey.as_bytes());
    H160::from_slice(h.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::traits::{CollectionKey, StorageRead, StorageWrite};
    use crate::storage::Storage;
    use crate::types::{
        parse_brc721_tx, Brc721Output, Brc721Payload, Brc721Token, OwnershipGroup, SlotRange,
    };
    use bitcoin::hashes::hash160;
    use bitcoin::{
        absolute, transaction, Amount, Block, BlockHash, OutPoint, ScriptBuf, Sequence,
        Transaction, TxIn, TxOut, Txid,
    };
    use bitcoincore_rpc::Error as RpcError;
    use ethereum_types::H160;
    use std::collections::HashMap;

    #[derive(Clone)]
    struct MockRpc {
        txs: HashMap<Txid, Transaction>,
    }

    impl BitcoinRpc for MockRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            unimplemented!()
        }

        fn get_block_hash(&self, _height: u64) -> Result<BlockHash, RpcError> {
            unimplemented!()
        }

        fn get_block(&self, _hash: &BlockHash) -> Result<Block, RpcError> {
            unimplemented!()
        }

        fn get_raw_transaction(&self, txid: &Txid) -> Result<Transaction, RpcError> {
            Ok(self.txs.get(txid).expect("mock tx exists").clone())
        }

        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            unimplemented!()
        }
    }

    fn make_register_ownership_tx(
        prevout: OutPoint,
        payload: RegisterOwnershipData,
    ) -> Transaction {
        use bitcoin::script::Builder;

        let brc_out = Brc721Output::new(Brc721Payload::RegisterOwnership(payload))
            .into_txout()
            .expect("brc721 txout");

        Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: prevout,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: bitcoin::Witness::default(),
            }],
            output: vec![
                brc_out,
                TxOut {
                    value: Amount::from_sat(546),
                    script_pubkey: Builder::new()
                        .push_opcode(bitcoin::opcodes::all::OP_DUP)
                        .into_script(),
                },
            ],
        }
    }

    fn script_hash_h160(script_pubkey: &ScriptBuf) -> H160 {
        let h = hash160::Hash::hash(script_pubkey.as_bytes());
        H160::from_slice(h.as_ref())
    }

    #[test]
    fn digest_persists_ranges_and_enforces_no_overlap() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("register_ownership_test.db"));
        storage.init().expect("init db");

        let prev_script = bitcoin::script::Builder::new()
            .push_opcode(bitcoin::opcodes::all::OP_CHECKSIG)
            .into_script();
        let prev_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![],
            output: vec![TxOut {
                value: Amount::from_sat(1_000),
                script_pubkey: prev_script.clone(),
            }],
        };
        let prev_txid = prev_tx.compute_txid();

        let prevout = OutPoint {
            txid: prev_txid,
            vout: 0,
        };

        let rpc = MockRpc {
            txs: HashMap::from([(prev_txid, prev_tx.clone())]),
        };

        let collection_key = CollectionKey::new(840_000, 2);

        let tx = storage.begin_tx().expect("begin tx");
        tx.save_collection(collection_key.clone(), H160::default(), false)
            .expect("save collection");

        let payload1 = RegisterOwnershipData::new(
            collection_key.block_height,
            collection_key.tx_index,
            vec![OwnershipGroup {
                output_index: 1,
                ranges: vec![SlotRange { start: 0, end: 9 }],
            }],
        )
        .expect("payload1");

        let bitcoin_tx1 = make_register_ownership_tx(prevout, payload1.clone());
        let brc_tx1 = parse_brc721_tx(&bitcoin_tx1)
            .expect("parse")
            .expect("some brc tx");
        brc_tx1.validate().expect("validate");

        digest(&rpc, &payload1, &brc_tx1, &tx, 900_000, 0).expect("digest ok");

        let initial_owner_h160 = script_hash_h160(&prev_script);
        let token = Brc721Token::new(5, initial_owner_h160).expect("token");
        let registered_owner = tx
            .load_registered_owner_h160(&collection_key, &token)
            .expect("load owner");
        assert!(registered_owner.is_some());

        let expected_owner_h160 = script_hash_h160(&bitcoin_tx1.output[1].script_pubkey);
        assert_eq!(registered_owner.unwrap(), expected_owner_h160);

        // Second tx overlaps [5,15] with already registered [0,9] => rejected.
        let payload2 = RegisterOwnershipData::new(
            collection_key.block_height,
            collection_key.tx_index,
            vec![OwnershipGroup {
                output_index: 1,
                ranges: vec![SlotRange { start: 5, end: 15 }],
            }],
        )
        .expect("payload2");

        let bitcoin_tx2 = make_register_ownership_tx(prevout, payload2.clone());
        let brc_tx2 = parse_brc721_tx(&bitcoin_tx2)
            .expect("parse")
            .expect("some brc tx");
        brc_tx2.validate().expect("validate");

        let err = digest(&rpc, &payload2, &brc_tx2, &tx, 900_000, 1).unwrap_err();
        assert!(matches!(err, Brc721Error::TxError(_)));
    }
}
