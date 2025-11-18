use bitcoin::blockdata::script::Instruction;
use bitcoin::opcodes;
use bitcoin::script::{Builder, PushBytesBuf};
use bitcoin::{Amount, ScriptBuf, TxOut};

use super::{Brc721Command, Brc721Message, BRC721_CODE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brc721Output {
    pub value: Amount,
    pub script_pubkey: ScriptBuf,
}

impl Brc721Output {
    pub fn from_slice(payload: Brc721Message) -> Self {
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

    pub fn new(payload: &[u8]) -> Self {
        Self::from_slice(payload)
    }

    pub fn from_output(output: &TxOut) -> Option<Self> {
        let payload = extract_payload(&output.script_pubkey)?;
        let command = *payload.first()?;
        Brc721Command::try_from(command).ok()?;
        Some(Self {
            value: output.value,
            script_pubkey: output.script_pubkey.clone(),
        })
    }

    pub fn payload(&self) -> Option<Vec<u8>> {
        extract_payload(&self.script_pubkey)
    }

    pub fn command(&self) -> Option<Brc721Command> {
        let payload = self.payload()?;
        let byte = *payload.first()?;
        Brc721Command::try_from(byte).ok()
    }

    pub fn into_txout(self) -> TxOut {
        TxOut {
            value: self.value,
            script_pubkey: self.script_pubkey,
        }
    }
}

fn extract_payload(script: &ScriptBuf) -> Option<Vec<u8>> {
    let mut instructions = script.instructions();
    match instructions.next()? {
        Ok(Instruction::Op(opcodes::all::OP_RETURN)) => {}
        _ => return None,
    }
    match instructions.next()? {
        Ok(Instruction::Op(BRC721_CODE)) => {}
        _ => return None,
    }
    match instructions.next()? {
        Ok(Instruction::PushBytes(bytes)) => Some(bytes.as_bytes().to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Brc721Command;

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

    #[test]
    fn from_output_roundtrip() {
        let payload = [Brc721Command::RegisterCollection as u8, 0x10, 0x11];
        let txout = Brc721Output::new(&payload).into_txout();
        let parsed = Brc721Output::from_output(&txout).expect("valid brc721 output");
        assert_eq!(parsed.value, txout.value);
        assert_eq!(parsed.script_pubkey, txout.script_pubkey);
    }

    #[test]
    fn from_output_rejects_non_brc721() {
        let script = bitcoin::script::Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(PushBytesBuf::try_from(vec![0x01]).unwrap())
            .into_script();
        let txout = TxOut {
            value: Amount::from_sat(0),
            script_pubkey: script,
        };
        assert!(Brc721Output::from_output(&txout).is_none());
    }

    #[test]
    fn from_output_rejects_invalid_command() {
        let payload = [0xFF, 0x01, 0x02];
        let txout = Brc721Output::from_slice(&payload).into_txout();
        assert!(Brc721Output::from_output(&txout).is_none());
    }

    #[test]
    fn payload_returns_original_bytes() {
        let payload = [0x21u8, 0x22, 0x23];
        let output = Brc721Output::new(&payload);
        assert_eq!(output.payload().unwrap(), payload);
    }

    #[test]
    fn command_returns_brc721_command() {
        let payload = [Brc721Command::RegisterCollection as u8, 0x00, 0x01];
        let output = Brc721Output::new(&payload);
        assert_eq!(output.command(), Some(Brc721Command::RegisterCollection));
    }
}
