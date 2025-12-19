use crate::storage::traits::StorageTx;
use crate::types::Brc721Error;
use bitcoin::Block;

pub trait BlockParser<T: StorageTx> {
    fn parse_block(&self, tx: &T, block: &Block, height: u64) -> Result<(), Brc721Error>;
}
