use crate::storage::traits::StorageWrite;
use crate::types::Brc721Error;
use bitcoin::Block;

pub trait BlockParser<T: StorageWrite> {
    fn parse_block(&self, tx: &T, block: &Block, height: u64) -> Result<(), Brc721Error>;
}
