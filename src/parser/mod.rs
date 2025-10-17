use crate::types::{Brc721Command, BRC721_CODE};
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
            if let Some(Err(ref e)) = self.parse_tx(tx, tx_index) {
                log::warn!("{:?}", e);
            }
        }
    }

    pub fn parse_tx(&self, tx: &Transaction, tx_index: usize) -> Option<Result<(), Brc721Error>> {
        let output = get_op_return_output(tx)?;
        let script = &output.script_pubkey;
        log::debug!("tx[{}] opret={:?}", tx_index, script);

        let bytes = output.script_pubkey.clone().into_bytes();
        if bytes.len() < 3 {
            return None;
        }
        if bytes[0] != opcodes::OP_RETURN.to_u8() {
            return None;
        }
        if bytes[1] != BRC721_CODE {
            return None;
        }
        let command = match Brc721Command::try_from(bytes[2]) {
            Ok(cmd) => cmd,
            Err(_) => {
                log::warn!("Failed to parse Brc721Command from byte {}", bytes[2]);
                return Some(Err(Brc721Error::WrongCommand {
                    expected: 0,
                    found: bytes[2],
                }));
            }
        };

        let result = match command {
            Brc721Command::RegisterCollection => register_collection::digest(script),
        };
        Some(result)
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
