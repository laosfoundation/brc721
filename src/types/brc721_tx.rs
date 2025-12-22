use crate::types::{Brc721Error, Brc721OpReturnOutput, Brc721Payload};
use bitcoin::Transaction;

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
        self.payload().validate_in_tx(self.tx)
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
    use crate::types::{Brc721Command, RegisterCollectionData, BRC721_CODE};
    use bitcoin::absolute;
    use bitcoin::opcodes::all::OP_RETURN;
    use bitcoin::script::{Builder, PushBytesBuf};
    use bitcoin::{transaction, Amount, ScriptBuf, Transaction, TxOut};
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
        Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![],
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
}
