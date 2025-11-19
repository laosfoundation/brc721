use crate::types::Brc721Error;
use bitcoin::Block;

pub trait BlockParse {
    fn parse_block(&self, block: &Block, height: u64) -> Result<(), Brc721Error>;
}
