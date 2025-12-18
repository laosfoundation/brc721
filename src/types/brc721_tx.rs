use crate::types::{Brc721Error, Brc721Message, Brc721Output};
use bitcoin::Transaction;

/// A parsed BRC-721 transaction envelope.
///
/// Protocol rule: the BRC-721 `OP_RETURN` output must be `vout=0`.
///
/// Even though the message payload is carried by output 0, many commands are
/// multi-output by nature (e.g. ownership references other outputs). This
/// envelope keeps the full transaction available for validation/digestion.
pub struct Brc721Tx<'a> {
    op_return: Brc721Output,
    bitcoin_tx: &'a Transaction,
}

impl<'a> Brc721Tx<'a> {
    pub fn message(&self) -> &Brc721Message {
        self.op_return.message()
    }

    pub fn bitcoin_tx(&self) -> &'a Transaction {
        self.bitcoin_tx
    }
}

/// Parse a BRC-721 transaction envelope from a Bitcoin transaction.
///
/// Returns `Ok(None)` if `vout=0` is not a BRC-721 `OP_RETURN`.
pub fn parse_brc721_tx(bitcoin_tx: &Transaction) -> Result<Option<Brc721Tx<'_>>, Brc721Error> {
    let Some(first_tx_out) = bitcoin_tx.output.first() else {
        return Ok(None);
    };

    match Brc721Output::from_output(first_tx_out) {
        Ok(op_return) => Ok(Some(Brc721Tx {
            op_return,
            bitcoin_tx,
        })),
        Err(Brc721Error::InvalidPayload) => Ok(None),
        Err(e) => Err(e),
    }
}
