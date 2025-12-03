use std::collections::HashSet;

use crate::storage::traits::{CollectionKey, StorageRead, StorageWrite, TokenKey, TokenOwnership};
use crate::types::{Brc721Error, RegisterOwnershipData, SlotNumber, TokenId};
use bitcoin::hashes::{hash160, Hash};
use bitcoin::script::Instruction;
use bitcoin::{OutPoint, PublicKey, Transaction, TxIn};
use hex::ToHex;

pub fn digest<S: StorageRead + StorageWrite>(
    payload: &RegisterOwnershipData,
    storage: &S,
    tx: &Transaction,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let input0 = tx.input.first().ok_or(Brc721Error::MissingOwnershipInput)?;
    let owner_h160 = derive_owner_h160(input0)?;

    let (collection_height, collection_tx_idx) = payload.collection_key_parts();
    let collection_key = CollectionKey::new(collection_height, collection_tx_idx);
    ensure_collection_exists(storage, &collection_key)?;
    ensure_outputs_available(tx, payload)?;

    let txid = tx.compute_txid();
    let mut pending = Vec::new();
    let mut seen = HashSet::new();

    for group in &payload.groups {
        let vout = group.output_index as u32;
        let outpoint = OutPoint::new(txid, vout);
        for range in &group.slot_ranges {
            for slot_value in range.start.value()..=range.end.value() {
                let slot = SlotNumber::new(slot_value).expect("range validated");
                let token_id = TokenId::new(slot, owner_h160);
                if !seen.insert(token_id) {
                    return Err(Brc721Error::InvalidOwnershipPayload(
                        "duplicate slot assignment detected",
                    ));
                }
                let token_key = TokenKey::new(collection_key.clone(), token_id);
                ensure_token_unregistered(storage, &token_key)?;
                pending.push(TokenOwnership {
                    key: token_key,
                    owner_outpoint: outpoint,
                    registered_block_height: block_height,
                    registered_tx_index: tx_index,
                });
            }
        }
    }

    for record in pending {
        storage
            .save_token(&record)
            .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
    }

    Ok(())
}

fn ensure_collection_exists<S: StorageRead>(
    storage: &S,
    key: &CollectionKey,
) -> Result<(), Brc721Error> {
    let exists = storage
        .load_collection(key)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
    if exists.is_none() {
        return Err(Brc721Error::CollectionNotFound(key.to_string()));
    }
    Ok(())
}

fn ensure_outputs_available(
    tx: &Transaction,
    payload: &RegisterOwnershipData,
) -> Result<(), Brc721Error> {
    let total = tx.output.len();
    for group in &payload.groups {
        let requested = group.output_index as usize;
        if requested >= total {
            return Err(Brc721Error::OwnershipOutputMissing {
                requested,
                available: total,
            });
        }
    }
    Ok(())
}

fn ensure_token_unregistered<S: StorageRead>(
    storage: &S,
    key: &TokenKey,
) -> Result<(), Brc721Error> {
    let existing = storage
        .load_token(key)
        .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
    if existing.is_some() {
        let owner_hex = key.token_id.initial_owner().encode_hex::<String>();
        return Err(Brc721Error::TokenAlreadyRegistered {
            collection: key.collection.to_string(),
            slot: key.token_id.slot().value(),
            owner: owner_hex,
        });
    }
    Ok(())
}

fn derive_owner_h160(input: &TxIn) -> Result<[u8; 20], Brc721Error> {
    if let Some(pubkey) = pubkey_from_witness(input) {
        return Ok(hash160_from_pubkey(&pubkey));
    }
    if let Some(pubkey) = pubkey_from_script(&input.script_sig) {
        return Ok(hash160_from_pubkey(&pubkey));
    }
    Err(Brc721Error::OwnershipProofUnavailable)
}

fn pubkey_from_witness(input: &TxIn) -> Option<PublicKey> {
    input
        .witness
        .last()
        .and_then(|element| PublicKey::from_slice(element).ok())
}

fn pubkey_from_script(script: &bitcoin::ScriptBuf) -> Option<PublicKey> {
    let mut last_pubkey = None;
    for instr in script.instructions() {
        let Ok(Instruction::PushBytes(bytes)) = instr else {
            continue;
        };
        if let Ok(pk) = PublicKey::from_slice(bytes.as_bytes()) {
            last_pubkey = Some(pk);
        }
    }
    last_pubkey
}

fn hash160_from_pubkey(pubkey: &PublicKey) -> [u8; 20] {
    let serialized = pubkey.to_bytes();
    hash160::Hash::hash(&serialized).to_byte_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::traits::{CollectionKey, Storage, StorageTx, StorageWrite, TokenKey};
    use crate::storage::SqliteStorage;
    use crate::types::{OwnershipGroup, SlotRange};
    use ethereum_types::H160;
    use bitcoin::{Amount, OutPoint, PublicKey, ScriptBuf, Transaction, TxIn, TxOut, Witness};
    use tempfile::TempDir;
    use std::str::FromStr;

    fn sample_transaction() -> Transaction {
        let mut witness = Witness::new();
        witness.push(vec![0u8; 72]); // fake signature placeholder
        let pubkey = PublicKey::from_str(
            "0250863ad64a87ae8a2fe83c1af1a8403cb5559f3b0f7c38d4fc8fcd969f0f0c2a",
        )
        .unwrap();
        witness.push(pubkey.to_bytes());

        Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: bitcoin::Sequence(0xffffffff),
                witness,
            }],
            output: vec![
                TxOut {
                    value: Amount::from_sat(0),
                    script_pubkey: ScriptBuf::new(),
                },
                TxOut {
                    value: Amount::from_sat(1000),
                    script_pubkey: ScriptBuf::new(),
                },
            ],
        }
    }

    #[test]
    fn derive_owner_h160_from_witness() {
        let tx = sample_transaction();
        let input0 = &tx.input[0];
        let h160 = derive_owner_h160(input0).expect("hash160");
        assert_eq!(h160.len(), 20);
    }

    #[test]
    fn digest_registers_tokens() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("ownership.sqlite");
        let storage = SqliteStorage::new(&db_path);
        storage.init().unwrap();

        let collection_key = CollectionKey::new(5, 0);
        let tx = storage.begin_tx().unwrap();
        tx.save_collection(collection_key.clone(), H160::zero(), false)
            .unwrap();

        let register_data = RegisterOwnershipData {
            collection_block_height: collection_key.block_height,
            collection_tx_index: collection_key.tx_index,
            groups: vec![OwnershipGroup {
                output_index: 1,
                slot_ranges: vec![SlotRange::new(
                    SlotNumber::new(0).unwrap(),
                    SlotNumber::new(0).unwrap(),
                )
                .unwrap()],
            }],
        };

        let transaction = sample_transaction();
        digest(&register_data, &tx, &transaction, 100, 2).unwrap();

        let owner = derive_owner_h160(&transaction.input[0]).unwrap();
        let token_key = TokenKey::new(
            collection_key,
            TokenId::new(SlotNumber::new(0).unwrap(), owner),
        );
        let record = tx.load_token(&token_key).unwrap().unwrap();
        assert_eq!(record.owner_outpoint.vout, 1);
        assert_eq!(record.registered_block_height, 100);
        assert_eq!(record.registered_tx_index, 2);
        tx.commit().unwrap();
    }
}
