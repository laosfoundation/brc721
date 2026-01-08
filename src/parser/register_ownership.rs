use crate::bitcoin_rpc::BitcoinRpc;
use crate::storage::traits::{CollectionKey, StorageRead};
use crate::types::{Brc721Command, Brc721Error, Brc721Tx, RegisterOwnershipData};
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

    match storage
        .load_collection(&collection_key)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?
    {
        Some(_) => {
            match base_address {
                Some(base_address) => log::error!(
                    "register-ownership not supported yet (block {} tx {}, collection {}, groups={}, input0_prevout={:?}, base_address={:#x})",
                    block_height,
                    tx_index,
                    collection_key,
                    payload.groups.len(),
                    input0_prevout,
                    base_address
                ),
                None => log::error!(
                    "register-ownership not supported yet (block {} tx {}, collection {}, groups={}, input0_prevout={:?}, base_address=<unknown>)",
                    block_height,
                    tx_index,
                    collection_key,
                    payload.groups.len(),
                    input0_prevout
                ),
            }
            Err(Brc721Error::UnsupportedCommand {
                cmd: Brc721Command::RegisterOwnership,
            })
        }
        None => {
            match base_address {
                Some(base_address) => log::warn!(
                    "register-ownership references unknown collection {} (block {} tx {}, input0_prevout={:?}, base_address={:#x})",
                    collection_key,
                    block_height,
                    tx_index,
                    input0_prevout,
                    base_address
                ),
                None => log::warn!(
                    "register-ownership references unknown collection {} (block {} tx {}, input0_prevout={:?}, base_address=<unknown>)",
                    collection_key,
                    block_height,
                    tx_index,
                    input0_prevout
                ),
            }
            Ok(())
        }
    }
}
