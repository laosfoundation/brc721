use crate::bitcoin_rpc::BitcoinRpc;
use crate::storage::traits::{CollectionKey, OwnershipUtxoSave, StorageRead, StorageWrite};
use crate::types::{
    h160_from_script_pubkey, Brc721Error, Brc721Token, Brc721Tx, RegisterOwnershipData,
};
use ethereum_types::H160;

fn try_base_h160_from_input0<R: BitcoinRpc>(brc721_tx: &Brc721Tx<'_>, rpc: &R) -> Option<H160> {
    let input0 = brc721_tx.input0()?;
    let prevout = input0.previous_output;
    if prevout == bitcoin::OutPoint::null() {
        return None;
    }
    let prev_tx = rpc.get_raw_transaction(&prevout.txid).ok()?;
    let prev_txout = prev_tx.output.get(prevout.vout as usize)?;
    Some(h160_from_script_pubkey(&prev_txout.script_pubkey))
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
    let base_h160 = try_base_h160_from_input0(brc721_tx, rpc);
    let (base_h160_log, asset_ids) = match base_h160 {
        Some(base_h160) => (
            format!("{:#x}", base_h160),
            asset_ids_for_payload(payload, base_h160),
        ),
        None => ("<unknown>".to_string(), "<unknown>".to_string()),
    };

    let collection = storage
        .load_collection(&collection_key)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?;

    if collection.is_none() {
        log::warn!(
            "register-ownership references unknown collection {} (block {} tx {}, asset_ids={}, input0_prevout={:?}, base_address={})",
            collection_key,
            block_height,
            tx_index,
            asset_ids,
            input0_prevout,
            base_h160_log
        );
        return Ok(());
    }

    let Some(base_h160) = base_h160 else {
        log::error!(
            "register-ownership missing base address (block {} tx {}, collection {}, groups={}, input0_prevout={:?})",
            block_height,
            tx_index,
            collection_key,
            payload.groups.len(),
            input0_prevout
        );
        return Ok(());
    };

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
                .save_ownership_range(&txid, reg_vout, range.start, range.end)
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
