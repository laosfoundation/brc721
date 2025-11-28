use crate::storage::traits::StorageWrite;
use crate::types::{Brc721Error, RegisterOwnershipData};

pub fn digest<S: StorageWrite>(
    payload: &RegisterOwnershipData,
    _storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let mapping_summary: Vec<String> = payload
        .slot_mappings()
        .iter()
        .map(|mapping| {
            format!(
                "out {} => {} ranges",
                mapping.output_index(),
                mapping.slot_ranges().len()
            )
        })
        .collect();
    log::info!(
        "📑 Register ownership command observed at block {}, tx {} for collection {}:{} ({} outputs) [{}]",
        block_height,
        tx_index,
        payload.collection_id().block_height(),
        payload.collection_id().tx_index(),
        payload.slot_mappings().len(),
        mapping_summary.join(", ")
    );
    Ok(())
}
