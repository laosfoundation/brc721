use crate::types::{Brc721Error, Brc721OpReturnOutput, Brc721Payload};
use bitcoin::{Transaction, TxIn, Txid};

/// A parsed BRC-721 transaction envelope.
///
/// Protocol rule: the BRC-721 `OP_RETURN` output must be `vout=0`.
///
/// Even though the message payload is carried by the `OP_RETURN` output (`vout=0`), many commands are
/// multi-output by nature (e.g. ownership references other outputs). This
/// envelope keeps the full transaction available for validation/digestion.
pub struct Brc721Tx<'a> {
    op_return_output: Brc721OpReturnOutput,
    tx: &'a Transaction,
}

impl<'a> Brc721Tx<'a> {
    pub fn validate(&self) -> Result<(), Brc721Error> {
        if self.input0().is_none() {
            return Err(Brc721Error::TxError("tx has no inputs".to_string()));
        }
        self.payload().validate_in_tx(self.tx)
    }

    pub fn input0(&self) -> Option<&TxIn> {
        self.tx.input.first()
    }

    pub fn txid(&self) -> Txid {
        self.tx.compute_txid()
    }

    pub fn payload(&self) -> &Brc721Payload {
        self.op_return_output.payload()
    }
}

/// Parse a BRC-721 transaction envelope from a Bitcoin transaction.
///
/// Returns `Ok(None)` if `vout=0` is not a BRC-721 `OP_RETURN` output.
pub fn parse_brc721_tx(bitcoin_tx: &Transaction) -> Result<Option<Brc721Tx<'_>>, Brc721Error> {
    let Some(op_return_output_txout) = bitcoin_tx.output.first() else {
        return Ok(None);
    };

    match Brc721OpReturnOutput::from_output(op_return_output_txout) {
        Ok(op_return_output) => Ok(Some(Brc721Tx {
            op_return_output,
            tx: bitcoin_tx,
        })),
        Err(Brc721Error::InvalidPayload) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::register_ownership::{OwnershipGroup, SlotRange};
    use crate::types::{Brc721Command, RegisterCollectionData, BRC721_CODE};
    use crate::types::{Brc721Payload, RegisterOwnershipData};
    use bitcoin::absolute;
    use bitcoin::opcodes::all::OP_RETURN;
    use bitcoin::script::{Builder, PushBytesBuf};
    use bitcoin::Witness;
    use bitcoin::{transaction, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut};
    use ethereum_types::H160;

    fn brc721_txout_for_payload(payload: &Brc721Payload) -> TxOut {
        let bytes = payload.to_bytes();
        let pb = PushBytesBuf::try_from(bytes).expect("payload should fit pushbytes");
        let script = Builder::new()
            .push_opcode(OP_RETURN)
            .push_opcode(BRC721_CODE)
            .push_slice(pb)
            .into_script();
        TxOut {
            value: Amount::from_sat(0),
            script_pubkey: script,
        }
    }

    fn empty_txout() -> TxOut {
        TxOut {
            value: Amount::from_sat(0),
            script_pubkey: ScriptBuf::new(),
        }
    }

    fn dummy_tx(outputs: Vec<TxOut>) -> Transaction {
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
            output: outputs,
        }
    }

    #[test]
    fn parse_brc721_tx_accepts_op_return_at_vout0() {
        let payload = Brc721Payload::RegisterCollection(RegisterCollectionData {
            evm_collection_address: H160::from_low_u64_be(1),
            rebaseable: false,
        });
        let tx = dummy_tx(vec![brc721_txout_for_payload(&payload), empty_txout()]);
        let parsed = parse_brc721_tx(&tx).expect("parse should succeed");
        let parsed = parsed.expect("expected Some(Brc721Tx)");
        assert_eq!(
            parsed.payload().command(),
            Brc721Command::RegisterCollection
        );
    }

    #[test]
    fn parse_brc721_tx_rejects_op_return_not_at_vout0() {
        let payload = Brc721Payload::RegisterCollection(RegisterCollectionData {
            evm_collection_address: H160::from_low_u64_be(1),
            rebaseable: false,
        });
        let tx = dummy_tx(vec![empty_txout(), brc721_txout_for_payload(&payload)]);
        let parsed = parse_brc721_tx(&tx).expect("parse should succeed");
        assert!(parsed.is_none());
    }

    #[test]
    fn validate_register_ownership_allows_multiple_lots_single_owner_output() {
        let ownership = RegisterOwnershipData::new(
            840_000,
            2,
            vec![OwnershipGroup {
                ranges: vec![
                    SlotRange { start: 0, end: 9 },
                    SlotRange { start: 10, end: 19 },
                ],
            }],
        )
        .expect("valid ownership data");

        let payload = Brc721Payload::RegisterOwnership(ownership);
        let tx = dummy_tx(vec![brc721_txout_for_payload(&payload), empty_txout()]);
        let parsed = parse_brc721_tx(&tx)
            .expect("parse should succeed")
            .expect("expected Some(Brc721Tx)");

        parsed
            .validate()
            .expect("ownership output references should be valid");
    }

    #[test]
    fn brc721_tx_exposes_input0() {
        let payload = Brc721Payload::RegisterCollection(RegisterCollectionData {
            evm_collection_address: H160::from_low_u64_be(1),
            rebaseable: false,
        });

        let txin = TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::default(),
        };

        let tx = Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![txin],
            output: vec![brc721_txout_for_payload(&payload), empty_txout()],
        };

        let parsed = parse_brc721_tx(&tx)
            .expect("parse should succeed")
            .expect("expected Some(Brc721Tx)");

        let input0 = parsed.input0().expect("input0 must exist");
        assert_eq!(input0.previous_output, OutPoint::null());
    }
}
