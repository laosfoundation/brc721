use crate::storage::traits::{CollectionKey, StorageWrite};
use crate::types::{Brc721Error, Brc721Payload, Brc721Tx};

pub fn digest<S: StorageWrite>(
    brc721_tx: &Brc721Tx<'_>,
    storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let key = CollectionKey::new(block_height, tx_index);
    let Brc721Payload::RegisterCollection(payload) = brc721_tx.payload() else {
        return Err(Brc721Error::TxError(
            "expected RegisterCollection message".to_string(),
        ));
    };

    storage
        .save_collection(key, payload.evm_collection_address, payload.rebaseable)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
    Ok(())
}
