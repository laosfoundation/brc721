use crate::storage::traits::StorageWrite;
use crate::types::Brc721Error;
use bitcoin::Block;

pub trait BlockParser {
    type Tx: StorageWrite;
    fn parse_block(&self, tx: &Self::Tx, block: &Block, height: u64) -> Result<(), Brc721Error>;
}
