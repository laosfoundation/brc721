use crate::storage::traits::StorageWrite;
use crate::types::{Brc721Error, RegisterOwnershipData};
use bitcoin::Transaction;

pub fn digest<S: StorageWrite>(
    payload: &RegisterOwnershipData,
    _storage: &S,
    tx_data: &Transaction,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let output_count = tx_data.output.len();
    for group in &payload.groups {
        if group.output_index as usize >= output_count {
            return Err(Brc721Error::TxError(format!(
                "register-ownership output_index {} out of bounds (tx outputs={})",
                group.output_index, output_count
            )));
        }
    }

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
