use crate::types::{Brc721Command, BRC721_CODE};
use bitcoin::blockdata::opcodes::all as opcodes;
use bitcoin::blockdata::script::Instruction;
use bitcoin::Block;
use bitcoin::Transaction;
use bitcoin::TxOut;

mod create_collection;

use thiserror::Error;

/// Custom error type for errors related to bitcoin script operations.
#[derive(Debug, Error, PartialEq)]
pub enum Brc721Error {
    /// An instruction of the expected type was not found in the script.
    #[error("Instruction not found: `{0}`")]
    InstructionNotFound(String),

    /// An unexpected instruction was encountered during decoding.
    #[error("Unexpected instruction")]
    UnexpectedInstruction,

    /// The length of a push operation in the script does not match the expected size.
    #[error("Invalid length: `{0}`")]
    InvalidLength(String),

    /// An error occurred during decoding.
    #[error("Decoding error: `{0}`")]
    Decode(String),
}

// The Parser struct serves as a namespace for parsing logic
pub struct Parser;

impl Parser {
    /// Parse the given Bitcoin block for BRC-721 operations
    pub fn parse_block(&self, block: &Block) {
        // Iterate over all transactions in the block
        for (tx_index, tx) in block.txdata.iter().enumerate() {
            // Attempt to extract an OP_RETURN output from the transaction
            let output = match get_op_return_output(tx) {
                Some(val) => val,
                None => continue, // Skip if no target output found
            };

            let script = &output.script_pubkey;

            log::debug!("tx[{}] opret={:?}", tx_index, script);

            let bytes = output.script_pubkey.clone().into_bytes();
            if bytes.len() < 3 {
                return; // Script too short for further parsing
            }

            // Ensure the first byte is OP_RETURN
            if bytes[0] != opcodes::OP_RETURN.to_u8() {
                return;
            };
            // Check protocol code is present
            if bytes[1] != BRC721_CODE {
                return;
            };

            // Try parsing the command from the third byte
            let command = match Brc721Command::try_from(bytes[2]) {
                Ok(cmd) => cmd,
                Err(_) => return,
            };

            // Dispatch command handler.
            match command {
                Brc721Command::CreateCollection => create_collection::digest(script),
            }
        }
    }
}

/// Returns the first output of the transaction if it is an OP_RETURN output
pub fn get_op_return_output(tx: &Transaction) -> Option<&TxOut> {
    let out0 = tx.output.first()?;
    let mut it = out0.script_pubkey.instructions();
    match it.next()? {
        Ok(Instruction::Op(opcodes::OP_RETURN)) => Some(out0),
        _ => None,
    }
}
