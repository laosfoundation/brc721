use crate::storage::traits::{CollectionKey, StorageWrite};
use crate::types::{Brc721Error, RegisterCollectionData};

pub fn digest<S: StorageWrite>(
    payload: &RegisterCollectionData,
    storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let key = CollectionKey {
        id: format!("{}:{}", block_height, tx_index),
    };

    storage
        .save_collection(key, payload.evm_collection_address, payload.rebaseable)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
    Ok(())
}
