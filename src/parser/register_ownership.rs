use crate::storage::traits::{CollectionKey, StorageRead};
use crate::types::{Brc721Command, Brc721Error, Brc721Tx, RegisterOwnershipData};

pub fn digest<S: StorageRead>(
    payload: &RegisterOwnershipData,
    _brc721_tx: &Brc721Tx<'_>,
    storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let collection_key = CollectionKey::new(payload.collection_height, payload.collection_tx_index);

    match storage
        .load_collection(&collection_key)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?
    {
        Some(_) => {
            log::error!(
                "register-ownership not supported yet (block {} tx {}, collection {}, groups={})",
                block_height,
                tx_index,
                collection_key,
                payload.groups.len()
            );
            Err(Brc721Error::UnsupportedCommand {
                cmd: Brc721Command::RegisterOwnership,
            })
        }
        None => {
            log::warn!(
                "register-ownership references unknown collection {} (block {} tx {})",
                collection_key,
                block_height,
                tx_index
            );
            Ok(())
        }
    }
}

