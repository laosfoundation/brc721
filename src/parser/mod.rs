use crate::types::{Brc721Command, BRC721_CODE};
use bitcoin::blockdata::opcodes::all as opcodes;
use bitcoin::blockdata::script::Instruction;
use bitcoin::Block;
use bitcoin::Transaction;
use bitcoin::TxOut;

mod create_collection;

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum Brc721Error {
    #[error("script too short")]
    ScriptTooShort,
    #[error("not OP_RETURN")]
    NotOpReturn,
    #[error("wrong protocol code: expected {expected:#x}, got {found:#x}")]
    WrongProtocolCode { expected: u8, found: u8 },
    #[error("wrong command: expected {expected:#x}, got {found:#x}")]
    WrongCommand { expected: u8, found: u8 },
    #[error("invalid rebase flag: {0}")]
    InvalidRebaseFlag(u8),
}

pub struct Parser;

impl Parser {
    pub fn parse_block(&self, block: &Block) {
        for (tx_index, tx) in block.txdata.iter().enumerate() {
            let output = match get_op_return_output(tx) {
                Some(val) => val,
                None => continue,
            };

            let script = &output.script_pubkey;

            log::debug!("tx[{}] opret={:?}", tx_index, script);

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
                Err(_) => {
                    log::warn!("Failed to parse Brc721Command from byte {}", bytes[2]);
                    return;
                }
            };

            let result = match command {
                Brc721Command::CreateCollection => create_collection::digest(script),
            };

            if let Err(ref e) = result {
                log::warn!("{:?}", e);
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
