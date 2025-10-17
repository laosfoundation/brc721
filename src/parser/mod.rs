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
    pub fn parse_block(&self, block: &Block) -> Result<(), Brc721Error> {
        for (_, tx) in block.txdata.iter().enumerate() {
            let output = get_op_return_output(tx).unwrap();

            let brc721_tx = match Self::get_brc721_tx(output) {
                Some(tx) => tx,
                None => return Ok(()),
            };

            if let Some(Err(ref e)) = Self::parse_brc721_tx(brc721_tx) {
                log::warn!("{:?}", e);
            }
        }
        Ok(())
    }

    pub fn get_brc721_tx(output: &TxOut) -> Option<&[u8]> {
        let mut it = output.script_pubkey.instructions();
        match it.next()? {
            Ok(Instruction::Op(opcodes::OP_RETURN)) => {}
            _ => return None,
        }
        match it.next()? {
            Ok(Instruction::Op(opcodes::OP_PUSHNUM_15)) => {}
            _ => return None,
        }
        match it.next()? {
            Ok(Instruction::PushBytes(payload)) => return Some(payload.as_bytes()),
            _ => return None,
        }
    }

    pub fn parse_brc721_tx(tx: &[u8]) -> Option<Result<(), Brc721Error>> {
        if tx.is_empty() {
            return None;
        }

        let command = match Brc721Command::try_from(tx[0]) {
            Ok(cmd) => cmd,
            Err(_) => {
                log::warn!("Failed to parse Brc721Command from byte {}", tx[0]);
                return Some(Err(Brc721Error::WrongCommand {
                    expected: 0,
                    found: tx[0],
                }));
            }
        };

        let result = match command {
            Brc721Command::RegisterCollection => register_collection::digest(tx),
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
