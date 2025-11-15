use crate::storage::Storage;
use crate::storage::traits::CollectionKey;
use crate::types::{Brc721Tx, RegisterCollectionMessage};

use super::Brc721Error;

pub fn digest(tx: &Brc721Tx, storage: &dyn Storage, block_height: u64, tx_index: u32) -> Result<(), Brc721Error> {
    let payload = RegisterCollectionMessage::decode(tx)?;
    let key = CollectionKey {
        block_height,
        tx_index,
    };
    let owner = format!("0x{:x}", payload.collection_address);
    let params = format!("rebaseable:{}", payload.rebaseable);
    storage
        .save_collection(key, owner, params)
        .map_err(|_| Brc721Error::ScriptTooShort)?;
    Ok(())
}
