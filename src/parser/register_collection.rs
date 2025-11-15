use crate::storage::Storage;
use crate::storage::traits::CollectionKey;
use crate::types::{Brc721Tx, RegisterCollectionMessage};

use super::Brc721Error;

pub fn digest(tx: &Brc721Tx, storage: &dyn Storage, block_height: u64, txid: &str) -> Result<(), Brc721Error> {
    let payload = RegisterCollectionMessage::decode(tx)?;
    let key = CollectionKey {
        block_height,
        txid: txid.to_string(),
    };
    let owner = format!("0x{:x}", payload.collection_address);
    let params = format!("rebaseable:{}", payload.rebaseable);
    storage
        .save_collection(key, owner, params)
        .map_err(|_| Brc721Error::ScriptTooShort)?;
    Ok(())
}
