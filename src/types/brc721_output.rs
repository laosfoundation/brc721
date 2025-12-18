use super::{Brc721Payload, BRC721_CODE};
use crate::types::Brc721Error;
use bitcoin::blockdata::script::Instruction;
use bitcoin::opcodes;
use bitcoin::script::{Builder, PushBytesBuf};
use bitcoin::{Amount, ScriptBuf, TxOut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brc721Output {
    value: Amount,
    payload: Brc721Payload,
}

impl Brc721Output {
    pub fn new(payload: Brc721Payload) -> Self {
        Self {
            value: Amount::from_sat(0),
            payload,
        }
    }

    pub fn from_output(output: &TxOut) -> Result<Self, Brc721Error> {
        let payload = extract_payload(&output.script_pubkey).ok_or(Brc721Error::InvalidPayload)?;
        let brc721_payload = Brc721Payload::try_from(payload.as_slice())?;
        Ok(Self {
            value: output.value,
            payload: brc721_payload,
        })
    }

    pub fn into_txout(self) -> Result<TxOut, Brc721Error> {
        let bytes = self.payload.to_bytes();
        let pb = PushBytesBuf::try_from(bytes).map_err(|_| Brc721Error::InvalidPayload)?;
        let script = Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_opcode(BRC721_CODE)
            .push_slice(pb)
            .into_script();
        Ok(TxOut {
            value: self.value,
            script_pubkey: script,
        })
    }

    pub fn payload(&self) -> &Brc721Payload {
        &self.payload
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
    use crate::types::{Brc721Command, RegisterCollectionData};
    use bitcoin::script::Builder;
    use bitcoin::{Amount, ScriptBuf, TxOut};
    use ethereum_types::H160;

    fn build_register_collection_payload() -> Vec<u8> {
        let addr = H160::from_low_u64_be(42);
        let data = RegisterCollectionData {
            evm_collection_address: addr,
            rebaseable: true,
        };
        let mut payload = Vec::with_capacity(1 + RegisterCollectionData::LEN);
        payload.push(Brc721Command::RegisterCollection as u8);
        payload.extend_from_slice(&data.to_bytes());
        payload
    }

    fn build_brc721_script_from_bytes(bytes: Vec<u8>) -> ScriptBuf {
        let pb = PushBytesBuf::try_from(bytes).expect("valid pushbytes for test");
        Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_opcode(BRC721_CODE)
            .push_slice(pb)
            .into_script()
    }

    #[test]
    fn extract_payload_ok_for_valid_script() {
        let payload = build_register_collection_payload();
        let script = build_brc721_script_from_bytes(payload.clone());

        let extracted = extract_payload(&script).expect("payload should be found");
        assert_eq!(extracted, payload);
    }

    #[test]
    fn extract_payload_rejects_non_op_return() {
        let script = Builder::new()
            .push_opcode(opcodes::all::OP_1SUB) // non OP_RETURN
            .into_script();

        assert!(extract_payload(&script).is_none());
    }

    #[test]
    fn extract_payload_rejects_wrong_marker_opcode() {
        let script = Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_opcode(opcodes::all::OP_1SUB) // non BRC721_CODE
            .into_script();

        assert!(extract_payload(&script).is_none());
    }

    #[test]
    fn from_output_roundtrip_ok() {
        // 1) Build a valid payload
        let payload = build_register_collection_payload();
        let brc721_payload =
            Brc721Payload::try_from(payload.as_slice()).expect("valid brc721 payload");

        // 2) Create a Brc721Output and convert it into a TxOut
        let output = Brc721Output::new(brc721_payload.clone());
        let txout = output.into_txout().expect("into_txout should succeed");

        // 3) Parse back from the TxOut
        let parsed = Brc721Output::from_output(&txout).expect("from_output should succeed");

        // 4) Check that the message matches
        assert_eq!(parsed.payload(), &brc721_payload);

        // 5) Check that the value is 0 sat (as defined by new())
        assert_eq!(txout.value, Amount::from_sat(0));
    }

    #[test]
    fn from_output_fails_on_invalid_script() {
        // Completely invalid script
        let script = ScriptBuf::new();
        let txout = TxOut {
            value: Amount::from_sat(0),
            script_pubkey: script,
        };

        let res = Brc721Output::from_output(&txout);
        match res {
            Err(Brc721Error::InvalidPayload) => {}
            other => panic!("expected InvalidPayload, got {:?}", other),
        }
    }

    #[test]
    fn from_output_fails_on_invalid_message_payload() {
        // Valid header (OP_RETURN + BRC721_CODE) but payload too short
        let bytes = vec![Brc721Command::RegisterCollection as u8]; // only the command, no data
        let script = build_brc721_script_from_bytes(bytes);

        let txout = TxOut {
            value: Amount::from_sat(0),
            script_pubkey: script,
        };

        let res = Brc721Output::from_output(&txout);

        // This depends on how Brc721Error is modeled,
        // but the idea is that it propagates the error from Brc721Message::from_bytes
        match res {
            Err(Brc721Error::InvalidLength(_, _)) => {}
            Err(e) => panic!("expected InvalidLength(..), got {:?}", e),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}
