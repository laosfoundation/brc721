use crate::storage::traits::{CollectionKey, StorageRead, StorageWrite};
use crate::types::{Brc721Error, Brc721Tx, RegisterOwnershipData};
use bitcoin::hashes::{hash160, Hash as _};
use ethereum_types::H160;

pub fn digest<S: StorageRead + StorageWrite>(
    payload: &RegisterOwnershipData,
    brc721_tx: &Brc721Tx<'_>,
    storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let collection_key = CollectionKey::new(payload.collection_height, payload.collection_tx_index);

    if storage
        .load_collection(&collection_key)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?
        .is_none()
    {
        log::warn!(
            "register-ownership references unknown collection {} (block {} tx {})",
            collection_key,
            block_height,
            tx_index
        );
        return Ok(());
    }

    // Reject overlapping ranges inside this command.
    let mut all_ranges: Vec<(u128, u128)> = payload
        .groups
        .iter()
        .flat_map(|group| group.ranges.iter().map(|range| (range.start, range.end)))
        .collect();
    all_ranges.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    if all_ranges.len() > 1 {
        let mut last = all_ranges[0];
        for current in all_ranges.iter().copied().skip(1) {
            if current.0 <= last.1 {
                log::warn!(
                    "register-ownership contains overlapping slots for collection {} (block {} tx {}), skipping",
                    collection_key,
                    block_height,
                    tx_index
                );
                return Ok(());
            }
            last = current;
        }
    }

    for (start, end) in &all_ranges {
        let overlap = storage
            .has_unspent_slot_overlap(&collection_key, *start, *end)
            .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
        if overlap {
            log::warn!(
                "register-ownership overlaps existing unspent slots for collection {} (block {} tx {}), skipping",
                collection_key,
                block_height,
                tx_index
            );
            return Ok(());
        }
    }

    let bitcoin_tx = brc721_tx.bitcoin_tx();
    let txid = bitcoin_tx.compute_txid();

    for (group_index, group) in payload.groups.iter().enumerate() {
        let output_index = group_index + 1;
        let txout = &bitcoin_tx.output[output_index];
        let script_hash = hash160::Hash::hash(txout.script_pubkey.as_bytes());
        let owner_h160 = H160::from_slice(script_hash.as_byte_array());

        let outpoint = bitcoin::OutPoint {
            txid,
            vout: output_index as u32,
        };

        for range in &group.ranges {
            storage
                .insert_ownership_range(
                    collection_key.clone(),
                    owner_h160,
                    outpoint,
                    range.start,
                    range.end,
                    block_height,
                    tx_index,
                )
                .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
        }
    }

    Ok(())
}
