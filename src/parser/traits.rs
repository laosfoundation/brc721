use crate::storage::traits::{StorageRead, StorageWrite};
use crate::types::Brc721Error;
use bitcoin::Block;

pub trait BlockParser<T: StorageRead + StorageWrite> {
    fn parse_block(&self, tx: &T, block: &Block, height: u64) -> Result<(), Brc721Error>;
}
