use crate::types::{Brc721Command, Brc721Tx, BRC721_CODE};
use bitcoin::blockdata::opcodes::all as opcodes;
use bitcoin::blockdata::script::Instruction;
use bitcoin::Block;
use bitcoin::Transaction;
use bitcoin::TxOut;

mod register_collection;

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum Brc721Error {
    #[error("script too short")]
    ScriptTooShort,
    #[error("wrong command: got {0}")]
    WrongCommand(u8),
    #[error("invalid rebase flag: {0}")]
    InvalidRebaseFlag(u8),
}

pub struct Parser;

impl Parser {
    pub fn parse_block(&self, block: &Block) -> Result<(), Brc721Error> {
        for tx in block.txdata.iter() {
            let output = match get_first_output_if_op_return(tx) {
                Some(output) => output,
                None => continue,
            };

            let brc721_tx = match get_brc721_tx(output) {
                Some(tx) => tx,
                None => continue,
            };

            if let Some(Err(ref e)) = digest(brc721_tx) {
                log::warn!("{:?}", e);
            }
        }
        Ok(())
    }
}

fn get_brc721_tx(output: &TxOut) -> Option<&[u8]> {
    let mut it = output.script_pubkey.instructions();
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_RETURN)) => {}
        _ => return None,
    }
    match it.next()? {
        Ok(Instruction::Op(BRC721_CODE)) => {}
        _ => return None,
    }
    match it.next()? {
        Ok(Instruction::PushBytes(payload)) => Some(payload.as_bytes()),
        _ => None,
    }
}

fn digest(tx: &Brc721Tx) -> Option<Result<(), Brc721Error>> {
    if tx.is_empty() {
        return None;
    }

    let command = match Brc721Command::try_from(tx[0]) {
        Ok(cmd) => cmd,
        Err(_) => {
            log::warn!("Failed to parse Brc721Command from byte {}", tx[0]);
            return Some(Err(Brc721Error::WrongCommand(tx[0])));
        }
    };

    let result = match command {
        Brc721Command::RegisterCollection => register_collection::digest(tx),
    };
    Some(result)
}

fn get_first_output_if_op_return(tx: &Transaction) -> Option<&TxOut> {
    let out0 = tx.output.first()?;
    let mut it = out0.script_pubkey.instructions();
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_RETURN)) => Some(out0),
        _ => None,
    }
}
