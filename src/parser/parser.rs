use crate::parser::register_collection;
use crate::types::{Brc721Command, Brc721Output};
use bitcoin::Block;

use super::Brc721Error;

pub struct Parser {
    storage: std::sync::Arc<dyn crate::storage::Storage + Send + Sync>,
}

impl Parser {
    pub fn new(storage: std::sync::Arc<dyn crate::storage::Storage + Send + Sync>) -> Self {
        Self { storage }
    }

    pub fn parse_block(&self, block: &Block, block_height: u64) -> Result<(), Brc721Error> {
        for (tx_index, tx) in block.txdata.iter().enumerate() {
            let Some(first_output) = tx.output.first() else {
                continue;
            };
            let brc721_output = match Brc721Output::from_output(first_output) {
                Some(output) => output,
                None => continue,
            };

            log::info!(
                "📦 Found BRC-721 tx at block {}, tx {}",
                block_height,
                tx_index
            );

            if let Some(Err(ref e)) = self.digest(&brc721_output, block_height, tx_index as u32) {
                log::warn!("{:?}", e);
            }
        }
        Ok(())
    }

    fn digest(
        &self,
        output: &Brc721Output,
        block_height: u64,
        tx_index: u32,
    ) -> Option<Result<(), Brc721Error>> {
        let command = match output.command() {
            Some(cmd) => cmd,
            None => return Some(Err(Brc721Error::ScriptTooShort)),
        };

        let result = match command {
            Brc721Command::RegisterCollection => register_collection::digest(
                output.message().as_slice(),
                self.storage.clone(),
                block_height,
                tx_index,
            ),
        };
        Some(result)
    }
}
