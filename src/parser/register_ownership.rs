use crate::storage::traits::StorageWrite;
use crate::types::{Brc721Error, RegisterOwnershipData};

pub fn digest<S: StorageWrite>(
    payload: &RegisterOwnershipData,
    _storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    log::info!(
        "📝 Valid register-ownership message at block {} tx {}, collection {}:{}, groups={}",
        block_height,
        tx_index,
        payload.collection_height,
        payload.collection_tx_index,
        payload.groups.len()
    );

    Ok(())
}
