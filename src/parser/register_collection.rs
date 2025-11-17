use crate::storage::traits::CollectionKey;
use crate::storage::Storage;
use crate::types::{Brc721Tx, RegisterCollectionMessage};

use super::Brc721Error;

pub fn digest(
    tx: &Brc721Tx,
    storage: std::sync::Arc<dyn Storage + Send + Sync>,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let payload = RegisterCollectionMessage::decode(tx)?;
    let key = CollectionKey {
        id: format!("{}:{}", block_height, tx_index),
    };
    let evm_collection_address = format!("0x{:x}", payload.evm_collection_address);
    let rebaseable = payload.rebaseable;
    storage
        .save_collection(key, evm_collection_address, rebaseable)
        .map_err(|_| Brc721Error::ScriptTooShort)?;
    Ok(())
}
