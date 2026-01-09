use crate::bitcoin_rpc::BitcoinRpc;
use crate::storage::traits::{CollectionKey, StorageRead};
use crate::types::{Brc721Command, Brc721Error, Brc721Token, Brc721Tx, RegisterOwnershipData};
use bitcoin::hashes::{hash160, Hash};
use ethereum_types::H160;

fn base_address_from_spent_script_pubkey(script_pubkey: &bitcoin::ScriptBuf) -> H160 {
    let hash = hash160::Hash::hash(script_pubkey.as_bytes());
    H160::from_slice(hash.as_byte_array())
}

fn try_base_address_from_input0<R: BitcoinRpc>(brc721_tx: &Brc721Tx<'_>, rpc: &R) -> Option<H160> {
    let input0 = brc721_tx.input0()?;
    let prevout = input0.previous_output;
    if prevout == bitcoin::OutPoint::null() {
        return None;
    }
    let prev_tx = rpc.get_raw_transaction(&prevout.txid).ok()?;
    let prev_txout = prev_tx.output.get(prevout.vout as usize)?;
    Some(base_address_from_spent_script_pubkey(
        &prev_txout.script_pubkey,
    ))
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

pub fn digest<S: StorageRead, R: BitcoinRpc>(
    payload: &RegisterOwnershipData,
    brc721_tx: &Brc721Tx<'_>,
    rpc: &R,
    storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let collection_key = CollectionKey::new(payload.collection_height, payload.collection_tx_index);

    let input0_prevout = brc721_tx.input0().map(|input0| input0.previous_output);
    let base_address = try_base_address_from_input0(brc721_tx, rpc);
    let (base_address_log, asset_ids) = match base_address {
        Some(base_address) => (
            format!("{:#x}", base_address),
            asset_ids_for_payload(payload, base_address),
        ),
        None => ("<unknown>".to_string(), "<unknown>".to_string()),
    };

    match storage
        .load_collection(&collection_key)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?
    {
        Some(_) => {
            log::error!(
                "register-ownership not supported yet (block {} tx {}, collection {}, groups={}, asset_ids={}, input0_prevout={:?}, base_address={})",
                block_height,
                tx_index,
                collection_key,
                payload.groups.len(),
                asset_ids,
                input0_prevout,
                base_address_log
            );
            Err(Brc721Error::UnsupportedCommand {
                cmd: Brc721Command::RegisterOwnership,
            })
        }
        None => {
            log::warn!(
                "register-ownership references unknown collection {} (block {} tx {}, asset_ids={}, input0_prevout={:?}, base_address={})",
                collection_key,
                block_height,
                tx_index,
                asset_ids,
                input0_prevout,
                base_address_log
            );
            Ok(())
        }
    }
}
