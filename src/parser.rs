use bitcoin::blockdata::opcodes::all as opcodes;
use bitcoin::blockdata::script::Instruction;
use bitcoin::Block;
use bitcoin::Script;

pub trait Parser {
    fn parse_block(&self, height: u64, block: &Block);
}

pub struct NoopParser;

pub fn op_return_first_pushdata(script: &Script) -> Option<Vec<u8>> {
    let mut it = script.instructions();
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_RETURN)) => {}
        _ => return None,
    }
    match it.next()? {
        Ok(Instruction::PushBytes(b)) => Some(b.as_bytes().to_vec()),
        Ok(Instruction::Op(opcodes::OP_PUSHBYTES_0)) => Some(Vec::new()),
        _ => None,
    }
}

fn first_op_return_push_hex(block: &Block) -> Option<String> {
    for tx in &block.txdata {
        for out in &tx.output {
            if let Some(bytes) = op_return_first_pushdata(out.script_pubkey.as_script()) {
                return Some(hex::encode(bytes));
            }
        }
    }
    None
}

impl Parser for NoopParser {
    fn parse_block(&self, height: u64, block: &Block) {
        let opret = first_op_return_push_hex(block).unwrap_or_else(|| "-".to_string());
        log::info!("🧱 block={} 🧾 txs={} 🔹 opret={}", height, block.txdata.len(), opret);
    }
}

pub fn parse_register_output0(script: &Script) -> Option<([u8; 20], bool)> {
    let mut it = script.instructions();
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_RETURN)) => {}
        _ => return None,
    }
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_PUSHNUM_15)) => {}
        _ => return None,
    }
    let flag_is_zero = match it.next()? {
        Ok(Instruction::Op(opcodes::OP_PUSHBYTES_0)) => true,
        Ok(Instruction::PushBytes(b)) => {
            let bytes = b.as_bytes();
            bytes.is_empty() || (bytes.len() == 1 && bytes[0] == 0)
        }
        _ => false,
    };
    if !flag_is_zero {
        return None;
    }
    let laos_bytes: [u8; 20] = match it.next()? {
        Ok(Instruction::PushBytes(b)) if b.as_bytes().len() == 20 => {
            let mut a = [0u8; 20];
            a.copy_from_slice(b.as_bytes());
            a
        }
        _ => return None,
    };
    let rebaseable = match it.next()? {
        Ok(Instruction::Op(opcodes::OP_PUSHBYTES_0)) => false,
        Ok(Instruction::Op(opcodes::OP_PUSHNUM_1)) => true,
        Ok(Instruction::PushBytes(b)) => {
            let bb = b.as_bytes();
            bb.len() == 1 && bb[0] != 0
        }
        _ => return None,
    };
    if it.next().is_some() {
        return None;
    }
    Some((laos_bytes, rebaseable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::ScriptBuf;

    fn script_with(flag: u8, laos: [u8; 20], reb: bool) -> ScriptBuf {
        let mut b = ScriptBuf::builder();
        b = b.push_opcode(opcodes::OP_RETURN);
        b = b.push_opcode(opcodes::OP_PUSHNUM_15);
        if flag == 0 {
            b = b.push_opcode(opcodes::OP_PUSHBYTES_0);
        } else if flag == 1 {
            b = b.push_opcode(opcodes::OP_PUSHNUM_1);
        } else {
            b = b.push_slice([flag]);
        }
        b = b.push_slice(laos);
        if reb {
            b = b.push_opcode(opcodes::OP_PUSHNUM_1);
        } else {
            b = b.push_opcode(opcodes::OP_PUSHBYTES_0);
        }
        b.into_script()
    }

    #[test]
    fn parse_register_output0_happy_path() {
        let mut laos = [0u8; 20];
        laos[0] = 0xaa;
        laos[19] = 0x55;
        let s = script_with(0, laos, true);
        let insts: Vec<String> = s
            .instructions()
            .map(|x| match x {
                Ok(Instruction::Op(op)) => format!("OP {}", op.to_u8()),
                Ok(Instruction::PushBytes(b)) => format!("PUSH {}", b.as_bytes().len()),
                Err(e) => format!("ERR {}", e),
            })
            .collect();
        log::debug!("insts={:?}", insts);
        let r = parse_register_output0(s.as_script());
        assert!(r.is_some());
        let (addr, rebaseable) = r.unwrap();
        assert_eq!(addr, laos);
        assert!(rebaseable);
    }

    #[test]
    fn parse_register_output0_requires_op_return() {
        let s = ScriptBuf::builder()
            .push_opcode(opcodes::OP_PUSHNUM_15)
            .push_opcode(opcodes::OP_PUSHBYTES_0)
            .push_slice([0u8; 20])
            .push_opcode(opcodes::OP_PUSHBYTES_0)
            .into_script();
        assert!(parse_register_output0(s.as_script()).is_none());
    }

    #[test]
    fn parse_register_output0_requires_flag_zero() {
        let s = script_with(1, [0u8; 20], false);
        assert!(parse_register_output0(s.as_script()).is_none());
    }

    #[test]
    fn parse_register_output0_requires_20b_address() {
        let s = ScriptBuf::builder()
            .push_opcode(opcodes::OP_RETURN)
            .push_opcode(opcodes::OP_PUSHNUM_15)
            .push_opcode(opcodes::OP_PUSHBYTES_0)
            .push_slice([0u8; 19])
            .push_opcode(opcodes::OP_PUSHBYTES_0)
            .into_script();
        assert!(parse_register_output0(s.as_script()).is_none());
    }

    #[test]
    fn op_return_first_pushdata_happy_path() {
        let s = ScriptBuf::builder()
            .push_opcode(opcodes::OP_RETURN)
            .push_slice([1u8, 2, 3])
            .into_script();
        let out = op_return_first_pushdata(s.as_script()).unwrap();
        assert_eq!(out, vec![1u8, 2, 3]);
    }

    #[test]
    fn op_return_first_pushdata_empty_push() {
        let s = ScriptBuf::builder()
            .push_opcode(opcodes::OP_RETURN)
            .push_opcode(opcodes::OP_PUSHBYTES_0)
            .into_script();
        let out = op_return_first_pushdata(s.as_script()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn op_return_first_pushdata_no_op_return() {
        let s = ScriptBuf::builder()
            .push_slice([1u8, 2, 3])
            .into_script();
        assert!(op_return_first_pushdata(s.as_script()).is_none());
    }

    #[test]
    fn op_return_first_pushdata_non_push_after_op_return() {
        let s = ScriptBuf::builder()
            .push_opcode(opcodes::OP_RETURN)
            .push_opcode(opcodes::OP_DROP)
            .into_script();
        assert!(op_return_first_pushdata(s.as_script()).is_none());
    }
}
