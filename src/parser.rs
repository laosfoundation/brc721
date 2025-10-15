use bitcoin::blockdata::opcodes::all as opcodes;
use bitcoin::blockdata::script::Instruction;
use bitcoin::Block;
use bitcoin::Script;
use bitcoin::Transaction;
use bitcoin::TxOut;

/// A block parser that can analyze blocks and perform domain-specific actions.
/// Implementations are injected into Core via dependency injection.
pub trait Parser {
    /// Parse a single block at the given height.
    /// Implementations should be side-effect free unless explicitly required.
    fn parse_block(&self, height: u64, block: &Block);
}

/// Default parser that only logs a short summary for each block.
pub struct NoopParser;

impl Parser for NoopParser {
    fn parse_block(&self, height: u64, block: &Block) {
        for (tx_index, tx) in block.txdata.iter().enumerate() {
            let has_opret = get_op_return_output(tx).is_some();
            log::info!(
                "🧱 block={} 🧾 tx[{}] 🔹opret={}",
                height,
                tx_index,
                has_opret,
            );
            if let Some(out0) = tx.output.first() {
                if let Some((laos, rebaseable)) =
                    parse_register_output0(out0.script_pubkey.as_script())
                {
                    let addr_hex = hex::encode(laos);
                    log::info!(
                        "✨ create-collection: height={} tx_index={} addr={} rebaseable={}",
                        height,
                        tx_index,
                        addr_hex,
                        rebaseable
                    );
                }
            }
        }
    }
}

/// A normalized script item after OP_RETURN: either an opcode (as u8) or raw push bytes.
#[derive(Debug, PartialEq, Eq)]
pub enum OpItem {
    Op(u8),
    Push(Vec<u8>),
}

/// Returns a normalized list of items that follow OP_RETURN in the script.
/// Returns None if the script does not start with OP_RETURN.
pub fn get_op_return_output(tx: &Transaction) -> Option<&TxOut> {
    let out0 = tx.output.first()?;
    let mut it = out0.script_pubkey.instructions();
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_RETURN)) => Some(out0),
        _ => None,
    }
}

pub fn op_return_items(script: &Script) -> Option<Vec<OpItem>> {
    let mut it = script.instructions();
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_RETURN)) => {}
        _ => return None,
    }
    let mut out = Vec::new();
    for instr in it {
        match instr.ok()? {
            Instruction::Op(op) => out.push(OpItem::Op(op.to_u8())),
            Instruction::PushBytes(b) => out.push(OpItem::Push(b.as_bytes().to_vec())),
        }
    }
    Some(out)
}

/// Parses a BRC-721 register script of the form:
/// OP_RETURN OP_15 <flag:0> <20-byte address> <rebaseable:0|1>
/// Returns (address20, rebaseable) on success, None otherwise.
pub fn parse_register_output0(script: &Script) -> Option<([u8; 20], bool)> {
    let items = op_return_items(script)?;
    if items.len() != 4 {
        return None;
    }
    match (&items[0], &items[1], &items[2], &items[3]) {
        (OpItem::Op(op), flag, OpItem::Push(addr), reb)
            if *op == opcodes::OP_PUSHNUM_15.to_u8() =>
        {
            let flag_is_zero = match flag {
                OpItem::Op(op) => *op == opcodes::OP_PUSHBYTES_0.to_u8(),
                OpItem::Push(b) => b.is_empty() || (b.len() == 1 && b[0] == 0),
            };
            if !flag_is_zero || addr.len() != 20 {
                return None;
            }
            let mut laos_bytes = [0u8; 20];
            laos_bytes.copy_from_slice(&addr[..]);
            let rebaseable = match reb {
                OpItem::Op(op) if *op == opcodes::OP_PUSHBYTES_0.to_u8() => false,
                OpItem::Op(op) if *op == opcodes::OP_PUSHNUM_1.to_u8() => true,
                OpItem::Push(b) => b.len() == 1 && b[0] != 0,
                _ => return None,
            };
            Some((laos_bytes, rebaseable))
        }
        _ => None,
    }
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
        let s = ScriptBuf::builder().push_slice([1u8, 2, 3]).into_script();
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

    #[test]
    fn op_return_items_parses_sequence() {
        let s = ScriptBuf::builder()
            .push_opcode(opcodes::OP_RETURN)
            .push_opcode(opcodes::OP_PUSHNUM_1)
            .push_slice([0xAA, 0xBB])
            .push_opcode(opcodes::OP_DROP)
            .into_script();
        let items = op_return_items(s.as_script()).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], OpItem::Op(opcodes::OP_PUSHNUM_1.to_u8()));
        assert_eq!(items[1], OpItem::Push(vec![0xAA, 0xBB]));
        assert_eq!(items[2], OpItem::Op(opcodes::OP_DROP.to_u8()));
    }

    #[test]
    fn get_op_return_output_on_vout0_returns_some() {
        use bitcoin::{OutPoint, Sequence, TxIn};
        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: bitcoin::Witness::default(),
            }],
            output: vec![bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(0),
                script_pubkey: ScriptBuf::builder()
                    .push_opcode(opcodes::OP_RETURN)
                    .push_opcode(opcodes::OP_PUSHBYTES_0)
                    .into_script(),
            }],
        };
        let out = get_op_return_output(&tx);
        assert!(out.is_some());
    }

    #[test]
    fn get_op_return_output_non_op_return_returns_none() {
        use bitcoin::{OutPoint, Sequence, TxIn};
        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: bitcoin::Witness::default(),
            }],
            output: vec![bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(0),
                script_pubkey: ScriptBuf::new(),
            }],
        };
        let out = get_op_return_output(&tx);
        assert!(out.is_none());
    }

    #[test]
    fn get_op_return_output_empty_outputs_returns_none() {
        use bitcoin::{OutPoint, Sequence, TxIn};
        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: bitcoin::Witness::default(),
            }],
            output: vec![],
        };
        let out = get_op_return_output(&tx);
        assert!(out.is_none());
    }
}
