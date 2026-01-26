use crate::bitcoin_rpc::BitcoinRpc;
use crate::storage::traits::{CollectionKey, OwnershipUtxoSave, StorageRead, StorageWrite};
use crate::types::{
    h160_from_script_pubkey, Brc721Error, Brc721Token, Brc721Tx, RegisterOwnershipData,
};
use ethereum_types::H160;

fn base_h160_from_input0<R: BitcoinRpc>(
    brc721_tx: &Brc721Tx<'_>,
    rpc: &R,
) -> Result<H160, Brc721Error> {
    let input0 = brc721_tx
        .input0()
        .ok_or_else(|| Brc721Error::TxError("register-ownership requires an input0".to_string()))?;
    let prevout = input0.previous_output;
    if prevout == bitcoin::OutPoint::null() {
        return Err(Brc721Error::TxError(
            "register-ownership input0 cannot be coinbase".to_string(),
        ));
    }
    let prev_tx = rpc.get_raw_transaction(&prevout.txid).map_err(|e| {
        Brc721Error::TxError(format!(
            "register-ownership requires txindex=1 or a node with access to input0; getrawtransaction({}) failed: {}",
            prevout.txid, e
        ))
    })?;
    let prev_txout = prev_tx.output.get(prevout.vout as usize).ok_or_else(|| {
        Brc721Error::TxError(format!(
            "register-ownership input0 vout {} out of range for tx {}",
            prevout.vout, prevout.txid
        ))
    })?;
    Ok(h160_from_script_pubkey(&prev_txout.script_pubkey))
}

fn token_id_decimal(slot_number: u128, base_address: H160) -> String {
    match Brc721Token::new(slot_number, base_address) {
        Ok(token) => token.to_u256().to_string(),
        Err(_) => format!("<invalid slot {}>", slot_number),
    }
}

fn asset_ids_for_payload(payload: &RegisterOwnershipData, base_address: H160) -> String {
    const MAX_ASSET_IDS_PER_GROUP: u128 = 32;

    payload
        .groups
        .iter()
        .enumerate()
        .map(|(group_index, group)| {
            let output_index = group_index + 1;

            let mut total_count = 0u128;
            let mut emitted_count = 0u128;
            let mut assets = Vec::new();

            for range in &group.ranges {
                let count = range.end - range.start + 1;
                total_count = total_count.saturating_add(count);

                if emitted_count >= MAX_ASSET_IDS_PER_GROUP {
                    continue;
                }

                let mut slot = range.start;
                loop {
                    if emitted_count >= MAX_ASSET_IDS_PER_GROUP {
                        break;
                    }
                    assets.push(token_id_decimal(slot, base_address));
                    emitted_count += 1;

                    if slot == range.end {
                        break;
                    }
                    slot += 1;
                }
            }

            if emitted_count < total_count {
                let remaining = total_count - emitted_count;
                assets.push(format!("...+{} more", remaining));
            }

            format!("vout{}=[{}]", output_index, assets.join(","))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn total_slots_in_payload(payload: &RegisterOwnershipData) -> u128 {
    payload
        .groups
        .iter()
        .flat_map(|group| group.ranges.iter())
        .fold(0u128, |acc, range| {
            acc.saturating_add(range.end.saturating_sub(range.start).saturating_add(1))
        })
}

pub fn digest<S: StorageRead + StorageWrite, R: BitcoinRpc>(
    payload: &RegisterOwnershipData,
    brc721_tx: &Brc721Tx<'_>,
    rpc: &R,
    storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    const MAX_REGISTERED_TOKENS_PER_TX: u128 = 1_000_000;

    let collection_key = CollectionKey::new(payload.collection_height, payload.collection_tx_index);
    let input0_prevout = brc721_tx.input0().map(|input0| input0.previous_output);

    let collection = storage
        .load_collection(&collection_key)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?;

    if collection.is_none() {
        log::warn!(
            "register-ownership references unknown collection {} (block {} tx {}, input0_prevout={:?})",
            collection_key,
            block_height,
            tx_index,
            input0_prevout
        );
        return Ok(());
    };

    let base_h160 = base_h160_from_input0(brc721_tx, rpc)?;
    let base_h160_log = format!("{:#x}", base_h160);
    let asset_ids = asset_ids_for_payload(payload, base_h160);

    let total_slots = total_slots_in_payload(payload);
    if total_slots > MAX_REGISTERED_TOKENS_PER_TX {
        log::error!(
            "register-ownership too many tokens (block {} tx {}, collection {}, token_count={}, max={}, input0_prevout={:?}, base_address={})",
            block_height,
            tx_index,
            collection_key,
            total_slots,
            MAX_REGISTERED_TOKENS_PER_TX,
            input0_prevout,
            base_h160_log
        );
        return Ok(());
    }

    let txid = brc721_tx.txid().to_string();

    for (group_index, group) in payload.groups.iter().enumerate() {
        let reg_vout: u32 = (group_index + 1).try_into().map_err(|_| {
            Brc721Error::TxError("register-ownership vout out of range".to_string())
        })?;

        let Some(owner_txout) = brc721_tx.output(reg_vout) else {
            log::error!(
                "register-ownership missing owner output (block {} tx {}, collection {}, reg_vout={})",
                block_height,
                tx_index,
                collection_key,
                reg_vout
            );
            return Ok(());
        };
        let owner_h160 = h160_from_script_pubkey(&owner_txout.script_pubkey);

        storage
            .save_ownership_utxo(OwnershipUtxoSave {
                collection_id: &collection_key,
                owner_h160,
                owner_script_pubkey: owner_txout.script_pubkey.as_bytes(),
                base_h160,
                reg_txid: &txid,
                reg_vout,
                created_height: block_height,
                created_tx_index: tx_index,
            })
            .map_err(|e| Brc721Error::StorageError(e.to_string()))?;

        for range in &group.ranges {
            if Brc721Token::new(range.start, base_h160).is_err()
                || Brc721Token::new(range.end, base_h160).is_err()
            {
                log::error!(
                    "register-ownership invalid slot range {}..={} at block {} tx {}",
                    range.start,
                    range.end,
                    block_height,
                    tx_index
                );
                return Ok(());
            }

            storage
                .save_ownership_range(
                    &txid,
                    reg_vout,
                    &collection_key,
                    base_h160,
                    range.start,
                    range.end,
                )
                .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
        }
    }

    log::info!(
        "register-ownership indexed (block {} tx {}, collection {}, token_count={}, groups={}, asset_ids={}, input0_prevout={:?}, base_address={})",
        block_height,
        tx_index,
        collection_key,
        total_slots,
        payload.groups.len(),
        asset_ids,
        input0_prevout,
        base_h160_log
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::traits::{
        Collection, CollectionKey, OwnershipRange, OwnershipRangeWithGroup, OwnershipUtxo,
        OwnershipUtxoSave, StorageRead, StorageWrite,
    };
    use crate::types::{Brc721OpReturnOutput, Brc721Payload, SlotRanges};
    use anyhow::Result as AnyResult;
    use bitcoin::blockdata::transaction::Version;
    use bitcoin::{
        absolute, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
    };
    use bitcoincore_rpc::Error as RpcError;
    use ethereum_types::H160;
    use std::str::FromStr;

    struct DummyRpc;

    impl BitcoinRpc for DummyRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            unimplemented!()
        }
        fn get_block_hash(&self, _height: u64) -> Result<bitcoin::BlockHash, RpcError> {
            unimplemented!()
        }
        fn get_block(&self, _hash: &bitcoin::BlockHash) -> Result<bitcoin::Block, RpcError> {
            unimplemented!()
        }
        fn get_raw_transaction(
            &self,
            _txid: &bitcoin::Txid,
        ) -> Result<bitcoin::Transaction, RpcError> {
            Err(RpcError::JsonRpc(bitcoincore_rpc::jsonrpc::Error::Rpc(
                bitcoincore_rpc::jsonrpc::error::RpcError {
                    code: -5,
                    message: "No such mempool or blockchain transaction. Use -txindex or provide a block hash.".into(),
                    data: None,
                },
            )))
        }
        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            unimplemented!()
        }
    }

    struct DummyStorage {
        collection: CollectionKey,
    }

    impl StorageRead for DummyStorage {
        fn load_last(&self) -> AnyResult<Option<crate::storage::Block>> {
            Ok(None)
        }
        fn load_collection(&self, id: &CollectionKey) -> AnyResult<Option<Collection>> {
            if id == &self.collection {
                Ok(Some(Collection {
                    key: self.collection.clone(),
                    evm_collection_address: H160::zero(),
                    rebaseable: false,
                }))
            } else {
                Ok(None)
            }
        }
        fn list_collections(&self) -> AnyResult<Vec<Collection>> {
            Ok(Vec::new())
        }
        fn list_unspent_ownership_utxos_by_outpoint(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> AnyResult<Vec<OwnershipUtxo>> {
            Ok(vec![])
        }
        fn list_unspent_ownership_ranges_by_outpoint(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> AnyResult<Vec<OwnershipRangeWithGroup>> {
            Ok(vec![])
        }
        fn list_ownership_ranges(&self, _utxo: &OwnershipUtxo) -> AnyResult<Vec<OwnershipRange>> {
            Ok(vec![])
        }
        fn find_unspent_ownership_utxo_for_slot(
            &self,
            _collection_id: &CollectionKey,
            _base_h160: H160,
            _slot: u128,
        ) -> AnyResult<Option<OwnershipUtxo>> {
            Ok(None)
        }
        fn list_unspent_ownership_utxos_by_owner(
            &self,
            _owner_h160: H160,
        ) -> AnyResult<Vec<OwnershipUtxo>> {
            Ok(vec![])
        }
    }

    impl StorageWrite for DummyStorage {
        fn save_last(&self, _height: u64, _hash: &str) -> AnyResult<()> {
            Ok(())
        }
        fn save_collection(
            &self,
            _key: CollectionKey,
            _evm_collection_address: H160,
            _rebaseable: bool,
        ) -> AnyResult<()> {
            Ok(())
        }
        fn save_ownership_utxo(&self, _utxo: OwnershipUtxoSave<'_>) -> AnyResult<()> {
            Ok(())
        }
        fn save_ownership_range(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _collection_id: &CollectionKey,
            _base_h160: H160,
            _slot_start: u128,
            _slot_end: u128,
        ) -> AnyResult<()> {
            Ok(())
        }
        fn mark_ownership_utxo_spent(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _spent_txid: &str,
            _spent_height: u64,
            _spent_tx_index: u32,
        ) -> AnyResult<()> {
            Ok(())
        }
    }

    #[test]
    fn register_ownership_requires_txindex_for_input0_lookup() {
        let slots = SlotRanges::from_str("0").expect("slots parse");
        let payload = RegisterOwnershipData::for_single_output(1, 2, slots)
            .expect("valid register ownership payload");
        let op_return =
            Brc721OpReturnOutput::new(Brc721Payload::RegisterOwnership(payload.clone()))
                .into_txout()
                .expect("opreturn txout");

        let prev_txid = bitcoin::Txid::from_str(&"11".repeat(32)).unwrap();
        let tx = Transaction {
            version: Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: prev_txid,
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::default(),
            }],
            output: vec![
                op_return,
                TxOut {
                    value: Amount::from_sat(546),
                    script_pubkey: ScriptBuf::new(),
                },
            ],
        };

        let brc721_tx = crate::types::parse_brc721_tx(&tx)
            .expect("parse should succeed")
            .expect("expected Some(Brc721Tx)");

        let storage = DummyStorage {
            collection: CollectionKey::new(1, 2),
        };
        let rpc = DummyRpc;

        let err = digest(&payload, &brc721_tx, &rpc, &storage, 10, 0).unwrap_err();
        assert!(format!("{err}").contains("txindex"));
    }
}
