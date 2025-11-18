use bitcoin::opcodes;
use bitcoin::script::{Builder, PushBytesBuf};
use bitcoin::{Amount, ScriptBuf, TxOut};

use super::BRC721_CODE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brc721Output {
    pub value: Amount,
    pub script_pubkey: ScriptBuf,
}

impl Brc721Output {
    pub fn new(payload: &[u8]) -> Self {
        let pb = PushBytesBuf::try_from(payload.to_vec()).unwrap();
        let script = Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_opcode(BRC721_CODE)
            .push_slice(pb)
            .into_script();
        Self {
            value: Amount::from_sat(0),
            script_pubkey: script,
        }
    }

    pub fn into_txout(self) -> TxOut {
        TxOut {
            value: self.value,
            script_pubkey: self.script_pubkey.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_output_contains_brc721_script() {
        let payload = [0xde, 0xad, 0xbe, 0xef];
        let output = Brc721Output::new(&payload);
        assert_eq!(output.value, Amount::from_sat(0));
        assert_eq!(
            output.script_pubkey.to_string(),
            "OP_RETURN OP_PUSHNUM_15 OP_PUSHBYTES_4 deadbeef"
        );
    }

    #[test]
    fn converts_into_txout() {
        let payload = [0x01, 0x02];
        let txout = Brc721Output::new(&payload).into_txout();
        assert_eq!(
            txout.script_pubkey.to_string(),
            "OP_RETURN OP_PUSHNUM_15 OP_PUSHBYTES_2 0102"
        );
    }
}
