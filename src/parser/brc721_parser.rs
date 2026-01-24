use crate::bitcoin_rpc::BitcoinRpc;
use crate::storage::traits::{
    CollectionKey, OwnershipRangeWithGroup, OwnershipUtxoSave, StorageRead, StorageWrite,
};
use crate::types::{
    h160_from_script_pubkey, parse_brc721_tx, Brc721Error, Brc721Payload, Brc721Tx,
};
use bitcoin::Block;
use bitcoin::Transaction;
use ethereum_types::H160;

use crate::parser::{BlockParser, TokenInput};

pub struct Brc721Parser;

fn unique_groups_from_ranges(ranges: &[OwnershipRangeWithGroup]) -> Vec<(CollectionKey, H160)> {
    let mut groups = Vec::new();
    for range in ranges {
        if groups.iter().any(|(collection_id, base_h160)| {
            *collection_id == range.collection_id && *base_h160 == range.base_h160
        }) {
            continue;
        }
        groups.push((range.collection_id.clone(), range.base_h160));
    }
    groups
}

fn merge_ordered_ranges(ranges: &[OwnershipRangeWithGroup]) -> Vec<OwnershipRangeWithGroup> {
    let mut out: Vec<OwnershipRangeWithGroup> = Vec::new();
    for range in ranges {
        if let Some(last) = out.last_mut() {
            if last.collection_id == range.collection_id && last.base_h160 == range.base_h160 {
                if let Some(next) = last.slot_end.checked_add(1) {
                    if next == range.slot_start {
                        last.slot_end = range.slot_end;
                        continue;
                    }
                }
            }
        }
        out.push(range.clone());
    }
    out
}

struct OutputOwnershipContext<'a> {
    txid: &'a str,
    vout: u32,
    owner_h160: H160,
    owner_script_pubkey: &'a [u8],
    block_height: u64,
    tx_index: u32,
}

fn save_ranges_for_output<S: StorageWrite>(
    storage: &S,
    ctx: OutputOwnershipContext<'_>,
    ranges: &[OwnershipRangeWithGroup],
) -> Result<(), Brc721Error> {
    let groups = unique_groups_from_ranges(ranges);
    for (collection_id, base_h160) in groups {
        storage
            .save_ownership_utxo(OwnershipUtxoSave {
                collection_id: &collection_id,
                owner_h160: ctx.owner_h160,
                owner_script_pubkey: ctx.owner_script_pubkey,
                base_h160,
                reg_txid: ctx.txid,
                reg_vout: ctx.vout,
                created_height: ctx.block_height,
                created_tx_index: ctx.tx_index,
            })
            .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
    }

    let merged = merge_ordered_ranges(ranges);
    for range in merged {
        storage
            .save_ownership_range(
                ctx.txid,
                ctx.vout,
                &range.collection_id,
                range.base_h160,
                range.slot_start,
                range.slot_end,
            )
            .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
    }

    Ok(())
}

impl Brc721Parser {
    pub fn new() -> Self {
        Self
    }

    fn parse_tx<T: StorageRead + StorageWrite, R: BitcoinRpc>(
        &self,
        storage: &T,
        rpc: &R,
        bitcoin_tx: &Transaction,
        block_height: u64,
        tx_index: u32,
    ) -> Result<(), Brc721Error> {
        let spend_txid = bitcoin_tx.compute_txid().to_string();

        let mut token_inputs: Vec<TokenInput> = Vec::new();
        let mut ownership_inputs_are_prefix = true;
        let mut saw_non_ownership_input = false;
        for txin in &bitcoin_tx.input {
            let prevout = txin.previous_output;
            if prevout == bitcoin::OutPoint::null() {
                continue;
            }

            let prev_txid = prevout.txid.to_string();
            let prev_vout = prevout.vout;

            let ranges = storage
                .list_unspent_ownership_ranges_by_outpoint(&prev_txid, prev_vout)
                .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
            if ranges.is_empty() {
                saw_non_ownership_input = true;
                continue;
            }

            if saw_non_ownership_input {
                ownership_inputs_are_prefix = false;
            }

            token_inputs.push(TokenInput {
                prev_txid,
                prev_vout,
                ranges,
            });
        }

        let input_count = bitcoin_tx
            .input
            .iter()
            .filter(|txin| txin.previous_output != bitcoin::OutPoint::null())
            .count();

        for txin in &bitcoin_tx.input {
            let prevout = txin.previous_output;
            if prevout == bitcoin::OutPoint::null() {
                continue;
            }

            if let Err(e) = storage.mark_ownership_utxo_spent(
                &prevout.txid.to_string(),
                prevout.vout,
                &spend_txid,
                block_height,
                tx_index,
            ) {
                log::error!(
                    "storage error marking ownership UTXO spent (outpoint={}:{}, spent_by={} at {}#{}, err={})",
                    prevout.txid,
                    prevout.vout,
                    spend_txid,
                    block_height,
                    tx_index,
                    e
                );
                return Err(Brc721Error::StorageError(e.to_string()));
            }
        }

        let brc721_tx: Option<Brc721Tx<'_>> = match parse_brc721_tx(bitcoin_tx) {
            Ok(Some(tx)) => Some(tx),
            Ok(None) => None,
            Err(e) => {
                log::warn!(
                    "Invalid BRC-721 message at block {} tx {}: {:?}",
                    block_height,
                    tx_index,
                    e
                );
                None
            }
        };

        if let Some(brc721_tx) = &brc721_tx {
            log::info!(
                "📦 Found BRC-721 tx at block {}, tx {} (txid={}, cmd={:?})",
                block_height,
                tx_index,
                bitcoin_tx.compute_txid(),
                brc721_tx.payload().command()
            );

            if let Brc721Payload::Mix(payload) = brc721_tx.payload() {
                let mix_ctx = crate::parser::mix::MixDigestContext {
                    token_inputs: &token_inputs,
                    input_count,
                    ownership_inputs_are_prefix,
                    storage,
                    block_height,
                    tx_index,
                };
                let handled = crate::parser::mix::digest(payload, brc721_tx, &mix_ctx)?;
                if handled {
                    return Ok(());
                }
                log::warn!(
                    "mix rejected; skipping implicit transfer (txid={})",
                    spend_txid
                );
                return Ok(());
            } else {
                self.digest_brc721_tx(storage, brc721_tx, block_height, tx_index, rpc)?;
            }
        }

        if token_inputs.is_empty() {
            return Ok(());
        }

        let remaining_outputs: Vec<(u32, &bitcoin::TxOut)> = bitcoin_tx
            .output
            .iter()
            .enumerate()
            .filter(|(_, out)| !out.script_pubkey.is_op_return())
            .map(|(vout, out)| (vout as u32, out))
            .collect();

        if remaining_outputs.is_empty() {
            // Burn: no valid outputs, assign tokens to the null owner on vout0 (provably unspendable OP_RETURN).
            let burn_vout = 0u32;
            let burn_script_pubkey = bitcoin_tx
                .output
                .get(burn_vout as usize)
                .map(|output| output.script_pubkey.as_bytes())
                .unwrap_or_else(|| &[]);
            for input in token_inputs {
                let group_count = unique_groups_from_ranges(&input.ranges).len();
                log::info!(
                    "🔥 Burning token input {}:{} (groups={}) at {}#{}",
                    input.prev_txid,
                    input.prev_vout,
                    group_count,
                    block_height,
                    tx_index
                );

                save_ranges_for_output(
                    storage,
                    OutputOwnershipContext {
                        txid: &spend_txid,
                        vout: burn_vout,
                        owner_h160: H160::zero(),
                        owner_script_pubkey: burn_script_pubkey,
                        block_height,
                        tx_index,
                    },
                    &input.ranges,
                )?;
            }

            return Ok(());
        }

        let remaining_outputs_len = remaining_outputs.len();
        for (input_index, input) in token_inputs.into_iter().enumerate() {
            let (dest_vout, dest_txout) = if input_index < remaining_outputs_len {
                remaining_outputs[input_index]
            } else {
                *remaining_outputs
                    .last()
                    .expect("non-empty remaining_outputs")
            };

            let dest_owner_h160 = h160_from_script_pubkey(&dest_txout.script_pubkey);
            let group_count = unique_groups_from_ranges(&input.ranges).len();

            log::info!(
                "🔁 Implicit transfer input {}:{} -> outpoint {}:{} (groups={}) at {}#{}",
                input.prev_txid,
                input.prev_vout,
                spend_txid,
                dest_vout,
                group_count,
                block_height,
                tx_index
            );

            save_ranges_for_output(
                storage,
                OutputOwnershipContext {
                    txid: &spend_txid,
                    vout: dest_vout,
                    owner_h160: dest_owner_h160,
                    owner_script_pubkey: dest_txout.script_pubkey.as_bytes(),
                    block_height,
                    tx_index,
                },
                &input.ranges,
            )?;
        }

        Ok(())
    }

    fn digest_brc721_tx<T: StorageRead + StorageWrite, R: BitcoinRpc>(
        &self,
        storage: &T,
        brc721_tx: &Brc721Tx<'_>,
        block_height: u64,
        tx_index: u32,
        rpc: &R,
    ) -> Result<(), Brc721Error> {
        brc721_tx.validate()?;

        match brc721_tx.payload() {
            Brc721Payload::RegisterCollection(payload) => {
                crate::parser::register_collection::digest(
                    payload,
                    brc721_tx,
                    storage,
                    block_height,
                    tx_index,
                )
            }
            Brc721Payload::RegisterOwnership(payload) => crate::parser::register_ownership::digest(
                payload,
                brc721_tx,
                rpc,
                storage,
                block_height,
                tx_index,
            ),
            Brc721Payload::Mix(_) => {
                log::warn!("mix payload must be handled by mix digest");
                Ok(())
            }
        }
    }
}

impl<T: StorageRead + StorageWrite> BlockParser<T> for Brc721Parser {
    fn parse_block<R: BitcoinRpc>(
        &self,
        storage: &T,
        block: &Block,
        block_height: u64,
        rpc: &R,
    ) -> Result<(), Brc721Error> {
        let hash = block.block_hash();
        let hash_str = hash.to_string();

        for (tx_index, bitcoin_tx) in block.txdata.iter().enumerate() {
            self.parse_tx(storage, rpc, bitcoin_tx, block_height, tx_index as u32)?;
        }
        // Persist last processed block once per block
        if let Err(e) = storage.save_last(block_height, &hash_str) {
            log::error!(
                "storage error saving block {} at height {}: {}",
                hash,
                block_height,
                e
            );
            return Err(Brc721Error::StorageError(e.to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::traits::{
        Block as StorageBlock, Collection, CollectionKey, OwnershipRange, OwnershipUtxo,
        OwnershipUtxoSave, StorageRead, StorageTx, StorageWrite,
    };
    use crate::storage::Storage;
    use crate::types::Brc721Command;
    use crate::types::BRC721_CODE;
    use anyhow::anyhow;
    use bitcoin::blockdata::constants::genesis_block;
    use bitcoin::hashes::Hash;
    use bitcoin::opcodes::all::OP_RETURN;
    use bitcoin::Network;
    use bitcoin::{Amount, Block, OutPoint, ScriptBuf, Transaction, TxIn, TxOut};
    use bitcoincore_rpc::Error as RpcError;
    use ethereum_types::H160;
    use hex::FromHex;
    use std::sync::{Arc, Mutex};

    struct DummyRpc;

    impl crate::bitcoin_rpc::BitcoinRpc for DummyRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            unimplemented!()
        }

        fn get_block_hash(&self, _height: u64) -> Result<bitcoin::BlockHash, RpcError> {
            unimplemented!()
        }

        fn get_block(&self, _hash: &bitcoin::BlockHash) -> Result<bitcoin::Block, RpcError> {
            unimplemented!()
        }

        fn get_raw_transaction(
            &self,
            _txid: &bitcoin::Txid,
        ) -> Result<bitcoin::Transaction, RpcError> {
            unimplemented!()
        }

        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            unimplemented!()
        }
    }

    fn build_payload(addr20: [u8; 20], rebase: u8) -> Vec<u8> {
        let mut v = Vec::with_capacity(1 + 20 + 1);
        v.push(Brc721Command::RegisterCollection as u8);
        v.extend_from_slice(&addr20);
        v.push(rebase);
        v
    }

    fn script_for_payload(payload: &[u8]) -> ScriptBuf {
        use bitcoin::script::Builder;
        Builder::new()
            .push_opcode(OP_RETURN)
            .push_opcode(BRC721_CODE)
            .push_slice(bitcoin::script::PushBytesBuf::try_from(payload.to_vec()).unwrap())
            .into_script()
    }

    #[test]
    fn test_script_hex_starts_with_6a5f16_and_matches_expected() {
        let addr = <[u8; 20]>::from_hex("ffff0123ffffffffffffffffffffffff3210ffff").unwrap();
        let payload = build_payload(addr, 0x00);
        let script = script_for_payload(&payload);
        let hex = hex::encode(script.as_bytes());
        assert_eq!(hex, "6a5f1600ffff0123ffffffffffffffffffffffff3210ffff00");
    }

    #[test]
    fn test_full_parse_flow_register_collection() {
        let addr = [0xABu8; 20];
        let payload = build_payload(addr, 0);
        let script = script_for_payload(&payload);
        let tx = Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: bitcoin::Sequence(0xffffffff),
                witness: bitcoin::Witness::default(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(0),
                script_pubkey: script,
            }],
        };
        let header = bitcoin::block::Header {
            version: bitcoin::block::Version::ONE,
            prev_blockhash: bitcoin::BlockHash::from_raw_hash(
                bitcoin::hashes::sha256d::Hash::all_zeros(),
            ),
            merkle_root: bitcoin::TxMerkleNode::from_raw_hash(
                bitcoin::hashes::sha256d::Hash::all_zeros(),
            ),
            time: 0,
            bits: bitcoin::CompactTarget::from_consensus(0),
            nonce: 0,
        };
        let block = Block {
            header,
            txdata: vec![tx],
        };
        let temp_dir = tempfile::tempdir().expect("create temp dir for db");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_parser_test.db"));
        storage.init().expect("init the database");
        let parser = Brc721Parser::new();
        let rpc = DummyRpc;
        let tx = storage.begin_tx().expect("init the tx");
        let r = parser.parse_block(&tx, &block, 0, &rpc);
        assert!(r.is_ok());
        tx.commit().unwrap();
    }

    #[test]
    fn implicit_transfer_maps_to_first_non_op_return_output() {
        use bitcoin::{absolute, transaction, PubkeyHash, Sequence, Witness};
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_implicit_1.db"));
        storage.init().expect("init db");

        let collection_id = CollectionKey::new(840_000, 0);
        let base_h160 = H160::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let owner_script = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner"));

        let prev_txid = bitcoin::Txid::from_str(
            "0101010101010101010101010101010101010101010101010101010101010101",
        )
        .unwrap();
        let prev_txid_str = prev_txid.to_string();
        let prev_vout = 7u32;

        let tx = storage.begin_tx().unwrap();
        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(1),
            owner_script_pubkey: owner_script.as_bytes(),
            base_h160,
            reg_txid: &prev_txid_str,
            reg_vout: prev_vout,
            created_height: 1,
            created_tx_index: 0,
        })
        .unwrap();
        tx.save_ownership_range(&prev_txid_str, prev_vout, &collection_id, base_h160, 0, 0)
            .unwrap();

        let op_return = bitcoin::script::Builder::new()
            .push_opcode(OP_RETURN)
            .into_script();

        let dest_script = ScriptBuf::new_p2pkh(&PubkeyHash::all_zeros());
        let dest_script_ignored = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"ignored"));

        let spending_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: prev_txid,
                    vout: prev_vout,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence(0xffffffff),
                witness: Witness::default(),
            }],
            output: vec![
                TxOut {
                    value: Amount::from_sat(0),
                    script_pubkey: op_return,
                },
                TxOut {
                    value: Amount::from_sat(1_000),
                    script_pubkey: dest_script.clone(),
                },
                TxOut {
                    value: Amount::from_sat(2_000),
                    script_pubkey: dest_script_ignored,
                },
            ],
        };
        let spend_txid_str = spending_tx.compute_txid().to_string();

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![spending_tx];
        let parser = Brc721Parser::new();
        let rpc = DummyRpc;
        parser.parse_block(&tx, &block, 100, &rpc).unwrap();
        tx.commit().unwrap();

        let prev_unspent = storage
            .list_unspent_ownership_utxos_by_outpoint(&prev_txid_str, prev_vout)
            .unwrap();
        assert!(prev_unspent.is_empty());

        let dest_utxos = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 1)
            .unwrap();
        assert_eq!(dest_utxos.len(), 1);
        assert_eq!(dest_utxos[0].collection_id, collection_id);
        assert_eq!(dest_utxos[0].base_h160, base_h160);
        assert_eq!(
            dest_utxos[0].owner_h160,
            crate::types::h160_from_script_pubkey(&dest_script)
        );

        let ranges = storage.list_ownership_ranges(&dest_utxos[0]).unwrap();
        assert_eq!(
            ranges,
            vec![OwnershipRange {
                slot_start: 0,
                slot_end: 0
            }]
        );

        let ignored = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 2)
            .unwrap();
        assert!(ignored.is_empty());
    }

    #[test]
    fn implicit_transfer_buckets_remaining_inputs_to_last_output() {
        use bitcoin::{absolute, transaction, PubkeyHash, Sequence, Witness};
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_implicit_2.db"));
        storage.init().expect("init db");

        let collection_id = CollectionKey::new(840_000, 0);

        let base_a = H160::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let base_b = H160::from_str("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let owner_script_a = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner_a"));
        let owner_script_b = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner_b"));

        let prev_txid_a = bitcoin::Txid::from_str(
            "1111111111111111111111111111111111111111111111111111111111111111",
        )
        .unwrap();
        let prev_txid_b = bitcoin::Txid::from_str(
            "2222222222222222222222222222222222222222222222222222222222222222",
        )
        .unwrap();

        let prev_a_str = prev_txid_a.to_string();
        let prev_b_str = prev_txid_b.to_string();

        let tx = storage.begin_tx().unwrap();
        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(1),
            owner_script_pubkey: owner_script_a.as_bytes(),
            base_h160: base_a,
            reg_txid: &prev_a_str,
            reg_vout: 0,
            created_height: 1,
            created_tx_index: 0,
        })
        .unwrap();
        tx.save_ownership_range(&prev_a_str, 0, &collection_id, base_a, 0, 0)
            .unwrap();

        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(2),
            owner_script_pubkey: owner_script_b.as_bytes(),
            base_h160: base_b,
            reg_txid: &prev_b_str,
            reg_vout: 1,
            created_height: 1,
            created_tx_index: 1,
        })
        .unwrap();
        tx.save_ownership_range(&prev_b_str, 1, &collection_id, base_b, 1, 1)
            .unwrap();

        let op_return = bitcoin::script::Builder::new()
            .push_opcode(OP_RETURN)
            .into_script();
        let dest_script = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest"));

        let spending_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![
                TxIn {
                    previous_output: OutPoint {
                        txid: prev_txid_a,
                        vout: 0,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence(0xffffffff),
                    witness: Witness::default(),
                },
                TxIn {
                    previous_output: OutPoint {
                        txid: prev_txid_b,
                        vout: 1,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence(0xffffffff),
                    witness: Witness::default(),
                },
            ],
            output: vec![
                TxOut {
                    value: Amount::from_sat(0),
                    script_pubkey: op_return,
                },
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script.clone(),
                },
            ],
        };
        let spend_txid_str = spending_tx.compute_txid().to_string();

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![spending_tx];
        let parser = Brc721Parser::new();
        let rpc = DummyRpc;
        parser.parse_block(&tx, &block, 101, &rpc).unwrap();
        tx.commit().unwrap();

        let dest_utxos = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 1)
            .unwrap();
        assert_eq!(dest_utxos.len(), 2);

        let mut bases = dest_utxos.iter().map(|u| u.base_h160).collect::<Vec<_>>();
        bases.sort();
        assert_eq!(bases, vec![base_a, base_b]);

        for utxo in &dest_utxos {
            assert_eq!(
                utxo.owner_h160,
                crate::types::h160_from_script_pubkey(&dest_script)
            );
        }

        let ranges_a = dest_utxos
            .iter()
            .find(|u| u.base_h160 == base_a)
            .map(|u| storage.list_ownership_ranges(u).unwrap())
            .unwrap();
        assert_eq!(
            ranges_a,
            vec![OwnershipRange {
                slot_start: 0,
                slot_end: 0
            }]
        );

        let ranges_b = dest_utxos
            .iter()
            .find(|u| u.base_h160 == base_b)
            .map(|u| storage.list_ownership_ranges(u).unwrap())
            .unwrap();
        assert_eq!(
            ranges_b,
            vec![OwnershipRange {
                slot_start: 1,
                slot_end: 1
            }]
        );
    }

    #[test]
    fn implicit_transfer_burns_when_no_valid_outputs() {
        use bitcoin::{absolute, transaction, Sequence, Witness};
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_implicit_3.db"));
        storage.init().expect("init db");

        let collection_id = CollectionKey::new(840_000, 0);
        let base_h160 = H160::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let owner_script = ScriptBuf::new();
        let prev_txid = bitcoin::Txid::from_str(
            "3333333333333333333333333333333333333333333333333333333333333333",
        )
        .unwrap();
        let prev_txid_str = prev_txid.to_string();

        let tx = storage.begin_tx().unwrap();
        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(1),
            owner_script_pubkey: owner_script.as_bytes(),
            base_h160,
            reg_txid: &prev_txid_str,
            reg_vout: 0,
            created_height: 1,
            created_tx_index: 0,
        })
        .unwrap();
        tx.save_ownership_range(&prev_txid_str, 0, &collection_id, base_h160, 7, 7)
            .unwrap();

        let op_return = bitcoin::script::Builder::new()
            .push_opcode(OP_RETURN)
            .into_script();

        let spending_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: prev_txid,
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence(0xffffffff),
                witness: Witness::default(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(0),
                script_pubkey: op_return,
            }],
        };
        let spend_txid_str = spending_tx.compute_txid().to_string();

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![spending_tx];
        let parser = Brc721Parser::new();
        let rpc = DummyRpc;
        parser.parse_block(&tx, &block, 102, &rpc).unwrap();
        tx.commit().unwrap();

        let burned = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 0)
            .unwrap();
        assert_eq!(burned.len(), 1);
        assert_eq!(burned[0].owner_h160, H160::zero());
        assert_eq!(burned[0].collection_id, collection_id);
        assert_eq!(burned[0].base_h160, base_h160);

        let ranges = storage.list_ownership_ranges(&burned[0]).unwrap();
        assert_eq!(
            ranges,
            vec![OwnershipRange {
                slot_start: 7,
                slot_end: 7
            }]
        );
    }

    #[test]
    fn mix_transfer_maps_indices_to_outputs() {
        use bitcoin::{absolute, transaction, PubkeyHash, Sequence, Witness};
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage = crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_mix.db"));
        storage.init().expect("init db");

        let collection_id = CollectionKey::new(840_000, 0);
        let base_h160 = H160::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let owner_script_a = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner_a"));
        let owner_script_b = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner_b"));

        let prev_txid_a = bitcoin::Txid::from_str(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
        let prev_txid_b = bitcoin::Txid::from_str(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .unwrap();
        let prev_a_str = prev_txid_a.to_string();
        let prev_b_str = prev_txid_b.to_string();

        let tx = storage.begin_tx().unwrap();
        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(1),
            owner_script_pubkey: owner_script_a.as_bytes(),
            base_h160,
            reg_txid: &prev_a_str,
            reg_vout: 0,
            created_height: 1,
            created_tx_index: 0,
        })
        .unwrap();
        tx.save_ownership_range(&prev_a_str, 0, &collection_id, base_h160, 0, 1)
            .unwrap();

        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(2),
            owner_script_pubkey: owner_script_b.as_bytes(),
            base_h160,
            reg_txid: &prev_b_str,
            reg_vout: 1,
            created_height: 1,
            created_tx_index: 1,
        })
        .unwrap();
        tx.save_ownership_range(&prev_b_str, 1, &collection_id, base_h160, 10, 11)
            .unwrap();

        let mix = crate::types::MixData::new(
            vec![
                vec![crate::types::mix::IndexRange { start: 2, end: 4 }],
                Vec::new(),
            ],
            1,
        )
        .expect("mix payload");
        let op_return =
            crate::types::Brc721OpReturnOutput::new(crate::types::Brc721Payload::Mix(mix))
                .into_txout()
                .unwrap();

        let dest_script_1 = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest1"));
        let dest_script_2 = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest2"));

        let spending_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![
                TxIn {
                    previous_output: OutPoint {
                        txid: prev_txid_a,
                        vout: 0,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence(0xffffffff),
                    witness: Witness::default(),
                },
                TxIn {
                    previous_output: OutPoint {
                        txid: prev_txid_b,
                        vout: 1,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence(0xffffffff),
                    witness: Witness::default(),
                },
            ],
            output: vec![
                op_return,
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script_1.clone(),
                },
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script_2.clone(),
                },
            ],
        };

        let spend_txid_str = spending_tx.compute_txid().to_string();

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![spending_tx];
        let parser = Brc721Parser::new();
        let rpc = DummyRpc;
        parser.parse_block(&tx, &block, 200, &rpc).unwrap();
        tx.commit().unwrap();

        let output1 = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 1)
            .unwrap();
        assert_eq!(output1.len(), 1);
        let ranges1 = storage.list_ownership_ranges(&output1[0]).unwrap();
        assert_eq!(
            ranges1,
            vec![OwnershipRange {
                slot_start: 10,
                slot_end: 11
            }]
        );

        let output2 = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 2)
            .unwrap();
        assert_eq!(output2.len(), 1);
        let ranges2 = storage.list_ownership_ranges(&output2[0]).unwrap();
        assert_eq!(
            ranges2,
            vec![OwnershipRange {
                slot_start: 0,
                slot_end: 1
            }]
        );
    }

    #[test]
    fn mix_allows_extra_inputs_after_ownership() {
        use bitcoin::{absolute, transaction, PubkeyHash, Sequence, Witness};
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_mix_extra.db"));
        storage.init().expect("init db");

        let collection_id = CollectionKey::new(840_000, 0);
        let base_h160 = H160::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let owner_script = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner"));

        let prev_txid = bitcoin::Txid::from_str(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
        let prev_txid_str = prev_txid.to_string();

        let tx = storage.begin_tx().unwrap();
        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(1),
            owner_script_pubkey: owner_script.as_bytes(),
            base_h160,
            reg_txid: &prev_txid_str,
            reg_vout: 0,
            created_height: 1,
            created_tx_index: 0,
        })
        .unwrap();
        tx.save_ownership_range(&prev_txid_str, 0, &collection_id, base_h160, 0, 1)
            .unwrap();

        let mix = crate::types::MixData::new(
            vec![
                vec![crate::types::mix::IndexRange { start: 1, end: 2 }],
                Vec::new(),
            ],
            1,
        )
        .expect("mix payload");
        let op_return =
            crate::types::Brc721OpReturnOutput::new(crate::types::Brc721Payload::Mix(mix))
                .into_txout()
                .unwrap();

        let dest_script_1 = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest1"));
        let dest_script_2 = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest2"));

        let funding_txid = bitcoin::Txid::from_str(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .unwrap();

        let spending_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![
                TxIn {
                    previous_output: OutPoint {
                        txid: prev_txid,
                        vout: 0,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence(0xffffffff),
                    witness: Witness::default(),
                },
                TxIn {
                    previous_output: OutPoint {
                        txid: funding_txid,
                        vout: 1,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence(0xffffffff),
                    witness: Witness::default(),
                },
            ],
            output: vec![
                op_return,
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script_1.clone(),
                },
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script_2.clone(),
                },
            ],
        };

        let spend_txid_str = spending_tx.compute_txid().to_string();

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![spending_tx];
        let parser = Brc721Parser::new();
        let rpc = DummyRpc;
        parser.parse_block(&tx, &block, 220, &rpc).unwrap();
        tx.commit().unwrap();

        let output1 = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 1)
            .unwrap();
        assert_eq!(output1.len(), 1);
        let ranges1 = storage.list_ownership_ranges(&output1[0]).unwrap();
        assert_eq!(
            ranges1,
            vec![OwnershipRange {
                slot_start: 1,
                slot_end: 1
            }]
        );

        let output2 = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 2)
            .unwrap();
        assert_eq!(output2.len(), 1);
        let ranges2 = storage.list_ownership_ranges(&output2[0]).unwrap();
        assert_eq!(
            ranges2,
            vec![OwnershipRange {
                slot_start: 0,
                slot_end: 0
            }]
        );
    }

    #[test]
    fn mix_rejects_non_ownership_inputs_before_ownership() {
        use bitcoin::{absolute, transaction, PubkeyHash, Sequence, Witness};
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage = crate::storage::SqliteStorage::new(
            temp_dir.path().join("brc721_mix_invalid_input_order.db"),
        );
        storage.init().expect("init db");

        let collection_id = CollectionKey::new(840_000, 0);
        let base_h160 = H160::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let owner_script = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner"));

        let prev_txid = bitcoin::Txid::from_str(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        )
        .unwrap();
        let prev_txid_str = prev_txid.to_string();

        let tx = storage.begin_tx().unwrap();
        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(1),
            owner_script_pubkey: owner_script.as_bytes(),
            base_h160,
            reg_txid: &prev_txid_str,
            reg_vout: 0,
            created_height: 1,
            created_tx_index: 0,
        })
        .unwrap();
        tx.save_ownership_range(&prev_txid_str, 0, &collection_id, base_h160, 0, 1)
            .unwrap();

        let mix = crate::types::MixData::new(
            vec![
                vec![crate::types::mix::IndexRange { start: 1, end: 2 }],
                Vec::new(),
            ],
            1,
        )
        .expect("mix payload");
        let op_return =
            crate::types::Brc721OpReturnOutput::new(crate::types::Brc721Payload::Mix(mix))
                .into_txout()
                .unwrap();

        let dest_script_1 = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest1"));
        let dest_script_2 = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest2"));

        let funding_txid = bitcoin::Txid::from_str(
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        )
        .unwrap();

        let spending_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![
                TxIn {
                    previous_output: OutPoint {
                        txid: funding_txid,
                        vout: 1,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence(0xffffffff),
                    witness: Witness::default(),
                },
                TxIn {
                    previous_output: OutPoint {
                        txid: prev_txid,
                        vout: 0,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence(0xffffffff),
                    witness: Witness::default(),
                },
            ],
            output: vec![
                op_return,
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script_1,
                },
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script_2,
                },
            ],
        };

        let spend_txid_str = spending_tx.compute_txid().to_string();

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![spending_tx];
        let parser = Brc721Parser::new();
        let rpc = DummyRpc;
        parser.parse_block(&tx, &block, 221, &rpc).unwrap();
        tx.commit().unwrap();

        let output1 = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 1)
            .unwrap();
        assert!(output1.is_empty());

        let output2 = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 2)
            .unwrap();
        assert!(output2.is_empty());
    }

    #[test]
    fn mix_preserves_input_order_across_groups() {
        use bitcoin::{absolute, transaction, PubkeyHash, Sequence, Witness};
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_mix_order.db"));
        storage.init().expect("init db");

        let collection_id = CollectionKey::new(840_000, 0);
        let base_a = H160::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let base_b = H160::from_str("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let owner_script_a = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner_a"));
        let owner_script_b = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner_b"));

        let prev_txid = bitcoin::Txid::from_str(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        )
        .unwrap();
        let prev_txid_str = prev_txid.to_string();

        let tx = storage.begin_tx().unwrap();
        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(1),
            owner_script_pubkey: owner_script_a.as_bytes(),
            base_h160: base_a,
            reg_txid: &prev_txid_str,
            reg_vout: 0,
            created_height: 1,
            created_tx_index: 0,
        })
        .unwrap();
        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(2),
            owner_script_pubkey: owner_script_b.as_bytes(),
            base_h160: base_b,
            reg_txid: &prev_txid_str,
            reg_vout: 0,
            created_height: 1,
            created_tx_index: 1,
        })
        .unwrap();

        // Insert ranges in an interleaved order: A0, B100, A1.
        tx.save_ownership_range(&prev_txid_str, 0, &collection_id, base_a, 0, 0)
            .unwrap();
        tx.save_ownership_range(&prev_txid_str, 0, &collection_id, base_b, 100, 100)
            .unwrap();
        tx.save_ownership_range(&prev_txid_str, 0, &collection_id, base_a, 1, 1)
            .unwrap();

        let mix = crate::types::MixData::new(
            vec![
                vec![crate::types::mix::IndexRange { start: 1, end: 2 }],
                Vec::new(),
            ],
            1,
        )
        .expect("mix payload");
        let op_return =
            crate::types::Brc721OpReturnOutput::new(crate::types::Brc721Payload::Mix(mix))
                .into_txout()
                .unwrap();

        let dest_script_1 = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest1"));
        let dest_script_2 = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest2"));

        let spending_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: prev_txid,
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence(0xffffffff),
                witness: Witness::default(),
            }],
            output: vec![
                op_return,
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script_1.clone(),
                },
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script_2.clone(),
                },
            ],
        };

        let spend_txid_str = spending_tx.compute_txid().to_string();

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![spending_tx];
        let parser = Brc721Parser::new();
        let rpc = DummyRpc;
        parser.parse_block(&tx, &block, 210, &rpc).unwrap();
        tx.commit().unwrap();

        let output1 = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 1)
            .unwrap();
        assert_eq!(output1.len(), 1);
        assert_eq!(output1[0].base_h160, base_b);
        let ranges1 = storage.list_ownership_ranges(&output1[0]).unwrap();
        assert_eq!(
            ranges1,
            vec![OwnershipRange {
                slot_start: 100,
                slot_end: 100
            }]
        );

        let output2 = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 2)
            .unwrap();
        assert_eq!(output2.len(), 1);
        assert_eq!(output2[0].base_h160, base_a);
        let ranges2 = storage.list_ownership_ranges(&output2[0]).unwrap();
        assert_eq!(
            ranges2,
            vec![OwnershipRange {
                slot_start: 0,
                slot_end: 1
            }]
        );
    }

    #[test]
    fn mix_invalid_does_not_fallback_to_implicit_transfer() {
        use bitcoin::{absolute, transaction, PubkeyHash, Sequence, Witness};
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage =
            crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_mix_invalid.db"));
        storage.init().expect("init db");

        let collection_id = CollectionKey::new(840_000, 0);
        let base_h160 = H160::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let owner_script = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner"));
        let prev_txid = bitcoin::Txid::from_str(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        )
        .unwrap();
        let prev_txid_str = prev_txid.to_string();

        let tx = storage.begin_tx().unwrap();
        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160: H160::from_low_u64_be(1),
            owner_script_pubkey: owner_script.as_bytes(),
            base_h160,
            reg_txid: &prev_txid_str,
            reg_vout: 0,
            created_height: 1,
            created_tx_index: 0,
        })
        .unwrap();
        tx.save_ownership_range(&prev_txid_str, 0, &collection_id, base_h160, 7, 7)
            .unwrap();

        let mix = crate::types::MixData::new(
            vec![
                vec![crate::types::mix::IndexRange { start: 0, end: 1 }],
                Vec::new(),
            ],
            1,
        )
        .expect("mix payload");
        let op_return =
            crate::types::Brc721OpReturnOutput::new(crate::types::Brc721Payload::Mix(mix))
                .into_txout()
                .unwrap();

        let dest_script = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"dest1"));

        // Invalid mix: payload declares 2 outputs, but the transaction only has vout0 + vout1.
        let spending_tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: prev_txid,
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence(0xffffffff),
                witness: Witness::default(),
            }],
            output: vec![
                op_return,
                TxOut {
                    value: Amount::from_sat(5_000),
                    script_pubkey: dest_script,
                },
            ],
        };

        let spend_txid_str = spending_tx.compute_txid().to_string();

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![spending_tx];
        let parser = Brc721Parser::new();
        let rpc = DummyRpc;
        parser.parse_block(&tx, &block, 201, &rpc).unwrap();
        tx.commit().unwrap();

        let output1 = storage
            .list_unspent_ownership_utxos_by_outpoint(&spend_txid_str, 1)
            .unwrap();
        assert!(output1.is_empty());
    }

    struct DummyStorageInner {
        last_height: Mutex<Option<u64>>,
        last_hash: Mutex<Option<String>>,
        fail: bool,
    }

    #[derive(Clone)]
    struct DummyStorage {
        inner: Arc<DummyStorageInner>,
    }

    impl DummyStorage {
        fn new(fail: bool) -> Self {
            Self {
                inner: Arc::new(DummyStorageInner {
                    last_height: Mutex::new(None),
                    last_hash: Mutex::new(None),
                    fail,
                }),
            }
        }
    }

    impl StorageTx for DummyStorage {
        fn commit(self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl Storage for DummyStorage {
        type Tx = DummyStorage;

        fn begin_tx(&self) -> anyhow::Result<Self::Tx> {
            Ok(self.clone())
        }
    }

    impl StorageRead for DummyStorage {
        fn load_last(&self) -> anyhow::Result<Option<StorageBlock>> {
            Ok(self
                .inner
                .last_height
                .lock()
                .unwrap()
                .map(|h| StorageBlock {
                    height: h,
                    hash: String::new(),
                }))
        }

        fn load_collection(&self, _id: &CollectionKey) -> anyhow::Result<Option<Collection>> {
            Ok(None)
        }

        fn list_collections(&self) -> anyhow::Result<Vec<Collection>> {
            Ok(Vec::new())
        }

        fn list_unspent_ownership_utxos_by_outpoint(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> anyhow::Result<Vec<OwnershipUtxo>> {
            Ok(vec![])
        }

        fn list_unspent_ownership_ranges_by_outpoint(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> anyhow::Result<Vec<OwnershipRangeWithGroup>> {
            Ok(vec![])
        }

        fn list_ownership_ranges(
            &self,
            _utxo: &OwnershipUtxo,
        ) -> anyhow::Result<Vec<OwnershipRange>> {
            Ok(vec![])
        }

        fn find_unspent_ownership_utxo_for_slot(
            &self,
            _collection_id: &CollectionKey,
            _base_h160: H160,
            _slot: u128,
        ) -> anyhow::Result<Option<OwnershipUtxo>> {
            Ok(None)
        }

        fn list_unspent_ownership_utxos_by_owner(
            &self,
            _owner_h160: H160,
        ) -> anyhow::Result<Vec<OwnershipUtxo>> {
            Ok(vec![])
        }
    }

    impl StorageWrite for DummyStorage {
        fn save_last(&self, height: u64, hash: &str) -> anyhow::Result<()> {
            if self.inner.fail {
                return Err(anyhow!("fail"));
            }
            *self.inner.last_height.lock().unwrap() = Some(height);
            *self.inner.last_hash.lock().unwrap() = Some(hash.to_string());
            Ok(())
        }

        fn save_collection(
            &self,
            _key: CollectionKey,
            _evm_collection_address: H160,
            _rebaseable: bool,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn save_ownership_utxo(&self, _utxo: OwnershipUtxoSave<'_>) -> anyhow::Result<()> {
            Ok(())
        }

        fn save_ownership_range(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _collection_id: &CollectionKey,
            _base_h160: H160,
            _slot_start: u128,
            _slot_end: u128,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn mark_ownership_utxo_spent(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _spent_txid: &str,
            _spent_height: u64,
            _spent_tx_index: u32,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn make_parser_with_storage(fail_storage: bool) -> (DummyStorage, Brc721Parser) {
        let storage = DummyStorage::new(fail_storage);
        let parser = Brc721Parser::new();
        (storage, parser)
    }

    #[test]
    fn parse_block_success_saves_height_and_hash() {
        let (storage, parser) = make_parser_with_storage(false);

        let block = genesis_block(Network::Regtest);
        let height = 42;
        let rpc = DummyRpc;
        let tx = storage.clone();
        parser.parse_block(&tx, &block, height, &rpc).unwrap();

        assert_eq!(*storage.inner.last_height.lock().unwrap(), Some(height));
        assert_eq!(
            *storage.inner.last_hash.lock().unwrap(),
            Some(block.block_hash().to_string())
        );
    }

    #[test]
    fn parse_block_storage_error_returns_error_and_does_not_persist() {
        let (storage, parser) = make_parser_with_storage(true);

        let block = genesis_block(Network::Regtest);
        let height = 1;
        let rpc = DummyRpc;
        let tx = storage.clone();
        assert!(parser.parse_block(&tx, &block, height, &rpc).is_err());

        assert_eq!(*storage.inner.last_height.lock().unwrap(), None);
        assert_eq!(*storage.inner.last_hash.lock().unwrap(), None);
    }

    #[test]
    fn parse_block_ignores_register_ownership_for_unknown_collection() {
        use crate::types::{Brc721OpReturnOutput, RegisterOwnershipData, SlotRanges};
        use std::str::FromStr;

        let (storage, parser) = make_parser_with_storage(false);

        let slots = SlotRanges::from_str("0").expect("slots parse");
        let ownership =
            RegisterOwnershipData::for_single_output(0, 0, slots).expect("ownership payload");

        let op_return = Brc721OpReturnOutput::new(Brc721Payload::RegisterOwnership(ownership))
            .into_txout()
            .expect("opreturn txout");

        let tx = Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: bitcoin::Sequence(0xffffffff),
                witness: bitcoin::Witness::default(),
            }],
            output: vec![
                op_return,
                TxOut {
                    value: Amount::from_sat(0),
                    script_pubkey: ScriptBuf::new(),
                },
            ],
        };

        let mut block = genesis_block(Network::Regtest);
        block.txdata = vec![tx];

        let height = 7;
        let rpc = DummyRpc;
        let tx = storage.clone();
        assert!(parser.parse_block(&tx, &block, height, &rpc).is_ok());
    }
}
