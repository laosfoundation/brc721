use crate::types::{Brc721Command, BRC721_CODE};
use bitcoin::blockdata::opcodes::all as opcodes;
use bitcoin::blockdata::script::Instruction;
use bitcoin::Block;
use bitcoin::Transaction;
use bitcoin::TxOut;

mod create_collection;

pub struct Parser;

impl Parser {
    pub fn parse_block(&self, block: &Block) {
        for (tx_index, tx) in block.txdata.iter().enumerate() {
            let output = match get_op_return_output(tx) {
                Some(val) => val,
                None => continue,
            };

            let script = &output.script_pubkey;

            log::debug!("🧾 tx[{}]🔹opret={:?}", tx_index, script);

            let bytes = output.script_pubkey.clone().into_bytes();
            if bytes.len() < 3 {
                return;
            }

            if bytes[0] != opcodes::OP_RETURN.to_u8() {
                return;
            };
            if bytes[1] != BRC721_CODE {
                return;
            };

            let command = match Brc721Command::try_from(bytes[2]) {
                Ok(cmd) => cmd,
                Err(_) => return,
            };

            match command {
                Brc721Command::CreateCollection => create_collection::digest(&script),
            }
        }
    }
}

pub fn get_op_return_output(tx: &Transaction) -> Option<&TxOut> {
    let out0 = tx.output.first()?;
    let mut it = out0.script_pubkey.instructions();
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_RETURN)) => Some(out0),
        _ => None,
    }
}
