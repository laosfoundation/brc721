use crate::storage::traits::{CollectionKey, StorageRead, StorageWrite};
use crate::types::{Brc721Error, Brc721Tx, RegisterCollectionData};

pub fn digest<S: StorageRead + StorageWrite>(
    payload: &RegisterCollectionData,
    _brc721_tx: &Brc721Tx<'_>,
    storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let existing = storage
        .list_collections()
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?
        .into_iter()
        .find(|collection| collection.evm_collection_address == payload.evm_collection_address);

    if let Some(existing) = existing {
        log::warn!(
            "register-collection already registered for evm_address={:#x} (existing={}, skipping {}:{})",
            payload.evm_collection_address,
            existing.key,
            block_height,
            tx_index
        );
        return Ok(());
    }

    let key = CollectionKey::new(block_height, tx_index);

    storage
        .save_collection(key, payload.evm_collection_address, payload.rebaseable)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SqliteStorage;
    use crate::storage::traits::{Storage, StorageTx};
    use crate::types::{parse_brc721_tx, Brc721OpReturnOutput, Brc721Payload};
    use bitcoin::absolute;
    use bitcoin::{transaction, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, Witness};
    use ethereum_types::H160;

    fn build_register_collection_tx(payload: RegisterCollectionData) -> Transaction {
        let op_return = Brc721OpReturnOutput::new(Brc721Payload::RegisterCollection(payload))
            .into_txout()
            .expect("build op_return output");
        let txin = TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::default(),
        };
        Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![txin],
            output: vec![op_return],
        }
    }

    #[test]
    fn register_collection_skips_duplicate() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage = SqliteStorage::new(temp_dir.path().join("dup_collection.db"));
        storage.init().expect("init db");

        let payload = RegisterCollectionData {
            evm_collection_address: H160::from_low_u64_be(42),
            rebaseable: false,
        };
        let collection_tx = build_register_collection_tx(payload.clone());
        let brc721_tx = parse_brc721_tx(&collection_tx)
            .expect("parse tx")
            .expect("brc721 tx");

        let db_tx = storage.begin_tx().expect("begin tx");

        digest(&payload, &brc721_tx, &db_tx, 100, 0).expect("first digest");
        digest(&payload, &brc721_tx, &db_tx, 101, 1).expect("second digest");
        db_tx.commit().expect("commit tx");

        let collections = storage.list_collections().expect("list collections");
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].key.to_string(), "100:0");
        assert_eq!(
            collections[0].evm_collection_address,
            payload.evm_collection_address
        );
    }
}
