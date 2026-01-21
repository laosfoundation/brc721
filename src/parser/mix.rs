use crate::parser::TokenInput;
use crate::storage::traits::{CollectionKey, OwnershipUtxoSave, StorageWrite};
use crate::types::{h160_from_script_pubkey, Brc721Error, Brc721Tx, MixData};
use ethereum_types::H160;

#[derive(Clone)]
struct InputSegment {
    collection_id: CollectionKey,
    base_h160: H160,
    slot_start: u128,
    index_start: u128,
    index_end: u128,
}

#[derive(Default)]
struct OutputAssignment {
    slices: Vec<OutputSlice>,
}

#[derive(Clone)]
struct OutputSlice {
    collection_id: CollectionKey,
    base_h160: H160,
    slot_start: u128,
    slot_end: u128,
}

impl OutputAssignment {
    fn push_slice(
        &mut self,
        collection_id: &CollectionKey,
        base_h160: H160,
        slot_start: u128,
        slot_end: u128,
    ) {
        if let Some(last) = self.slices.last_mut() {
            if last.collection_id == *collection_id
                && last.base_h160 == base_h160
            {
                if let Some(next) = last.slot_end.checked_add(1) {
                    if next == slot_start {
                        last.slot_end = slot_end;
                        return;
                    }
                }
            }
        }

        self.slices.push(OutputSlice {
            collection_id: collection_id.clone(),
            base_h160,
            slot_start,
            slot_end,
        });
    }

    fn unique_groups(&self) -> Vec<(CollectionKey, H160)> {
        let mut groups = Vec::new();
        for slice in &self.slices {
            if groups
                .iter()
                .any(|(collection_id, base_h160)| {
                    *collection_id == slice.collection_id && *base_h160 == slice.base_h160
                })
            {
                continue;
            }
            groups.push((slice.collection_id.clone(), slice.base_h160));
        }
        groups
    }
}

struct ExplicitRange {
    start: u128,
    end: u128,
    output_index: usize,
}

pub fn digest<S: StorageWrite>(
    payload: &MixData,
    brc721_tx: &Brc721Tx<'_>,
    token_inputs: &[TokenInput],
    input_count: usize,
    storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<bool, Brc721Error> {
    let txid = brc721_tx.txid().to_string();

    if let Err(err) = brc721_tx.validate() {
        log::warn!("mix validation failed (txid={}, err={})", txid, err);
        return Ok(false);
    }

    if token_inputs.is_empty() {
        log::warn!("mix has no ownership inputs (txid={})", txid);
        return Ok(false);
    }

    if input_count != token_inputs.len() {
        log::warn!(
            "mix inputs must all be ownership UTXOs (txid={}, input_count={}, ownership_inputs={})",
            txid,
            input_count,
            token_inputs.len()
        );
        return Ok(false);
    }

    let mut segments = Vec::new();
    let mut index_cursor: u128 = 0;

    for input in token_inputs {
        for range in &input.ranges {
            let len = range
                .slot_end
                .checked_sub(range.slot_start)
                .and_then(|delta| delta.checked_add(1))
                .ok_or_else(|| Brc721Error::TxError("mix input range length overflow".into()))?;

            let index_start = index_cursor;
            let index_end = index_cursor
                .checked_add(len)
                .ok_or_else(|| Brc721Error::TxError("mix index overflow".into()))?;

            segments.push(InputSegment {
                collection_id: range.collection_id.clone(),
                base_h160: range.base_h160,
                slot_start: range.slot_start,
                index_start,
                index_end,
            });
            index_cursor = index_end;
        }
    }

    let total_tokens = index_cursor;
    if let Err(err) = payload.validate_token_count(total_tokens) {
        log::warn!(
            "mix token range validation failed (txid={}, token_count={}, err={})",
            txid,
            total_tokens,
            err
        );
        return Ok(false);
    }

    let output_count = payload.output_ranges.len();
    for output_index in 0..output_count {
        let vout = (output_index + 1) as u32;
        let Some(output) = brc721_tx.output(vout) else {
            log::warn!(
                "mix missing output {} (txid={}, vout={})",
                output_index,
                txid,
                vout
            );
            return Ok(false);
        };
        if output.script_pubkey.is_op_return() {
            log::warn!(
                "mix output {} is op_return (txid={}, vout={})",
                output_index,
                txid,
                vout
            );
            return Ok(false);
        }
    }

    let mut explicit_ranges = Vec::new();
    for (output_index, ranges) in payload.output_ranges.iter().enumerate() {
        if output_index == payload.complement_index {
            continue;
        }
        for range in ranges {
            explicit_ranges.push(ExplicitRange {
                start: range.start,
                end: range.end,
                output_index,
            });
        }
    }
    explicit_ranges.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));

    let mut assignments = Vec::with_capacity(output_count);
    for _ in 0..output_count {
        assignments.push(OutputAssignment::default());
    }
    let mut range_idx = 0usize;

    for segment in segments {
        let mut cursor = segment.index_start;
        while cursor < segment.index_end {
            while range_idx < explicit_ranges.len() && cursor >= explicit_ranges[range_idx].end {
                range_idx += 1;
            }

            let (slice_end, output_index) = match explicit_ranges.get(range_idx) {
                Some(range) if cursor >= range.start => {
                    let end = segment.index_end.min(range.end);
                    (end, range.output_index)
                }
                Some(range) => {
                    let end = segment.index_end.min(range.start);
                    (end, payload.complement_index)
                }
                None => (segment.index_end, payload.complement_index),
            };

            if slice_end <= cursor {
                return Err(Brc721Error::TxError(
                    "mix slice did not advance cursor".into(),
                ));
            }

            let offset = cursor - segment.index_start;
            let slice_len = slice_end - cursor;
            let slot_start = segment
                .slot_start
                .checked_add(offset)
                .ok_or_else(|| Brc721Error::TxError("mix slot overflow".into()))?;
            let slot_end = slot_start
                .checked_add(slice_len - 1)
                .ok_or_else(|| Brc721Error::TxError("mix slot overflow".into()))?;

            assignments[output_index].push_slice(
                &segment.collection_id,
                segment.base_h160,
                slot_start,
                slot_end,
            );

            cursor = slice_end;
        }
    }

    for (output_index, assignment) in assignments.into_iter().enumerate() {
        if assignment.slices.is_empty() {
            continue;
        }

        let vout = (output_index + 1) as u32;
        let owner_h160 = brc721_tx
            .output(vout)
            .map(|output| h160_from_script_pubkey(&output.script_pubkey))
            .unwrap_or_else(H160::zero);

        for (collection_id, base_h160) in assignment.unique_groups() {
            storage
                .save_ownership_utxo(OwnershipUtxoSave {
                    collection_id: &collection_id,
                    owner_h160,
                    base_h160,
                    reg_txid: &txid,
                    reg_vout: vout,
                    created_height: block_height,
                    created_tx_index: tx_index,
                })
                .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
        }

        for slice in assignment.slices {
            storage
                .save_ownership_range(
                    &txid,
                    vout,
                    &slice.collection_id,
                    slice.base_h160,
                    slice.slot_start,
                    slice.slot_end,
                )
                .map_err(|e| Brc721Error::StorageError(e.to_string()))?;
        }
    }

    log::info!(
        "mix indexed (txid={}, inputs={}, outputs={}, token_count={}, complement_output={})",
        txid,
        input_count,
        output_count,
        total_tokens,
        payload.complement_index + 1
    );

    Ok(true)
}
