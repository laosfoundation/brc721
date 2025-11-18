use bitcoin::blockdata::script::Instruction;
use bitcoin::opcodes;
use bitcoin::script::{Builder, PushBytesBuf};
use bitcoin::{Amount, ScriptBuf, TxOut};

use super::{Brc721Command, Brc721Message, BRC721_CODE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brc721Output {
    pub value: Amount,
    message: Brc721Message,
}

impl Brc721Output {
    pub fn new(message: Brc721Message) -> Self {
        Self {
            value: Amount::from_sat(0),
            message,
        }
    }

    pub fn from_output(output: &TxOut) -> Option<Self> {
        let payload = extract_payload(&output.script_pubkey)?;
        let message = Brc721Message::from_bytes(&payload).ok()?;
        Some(Self {
            value: output.value,
            message,
        })
    }

    pub fn message(&self) -> &Brc721Message {
        &self.message
    }

    pub fn command(&self) -> Brc721Command {
        self.message.command()
    }

    pub fn into_txout(self) -> TxOut {
        let bytes = self.message.to_bytes();
        let pb = PushBytesBuf::try_from(bytes).unwrap();
        let script = Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_opcode(BRC721_CODE)
            .push_slice(pb)
            .into_script();
        TxOut {
            value: self.value,
            script_pubkey: script,
        }
    }
}

fn extract_payload(script: &ScriptBuf) -> Option<&[u8]> {
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
        Ok(Instruction::PushBytes(bytes)) => Some(bytes.as_bytes()),
        _ => None,
    }
}
