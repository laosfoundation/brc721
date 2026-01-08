use crate::bitcoin_rpc::BitcoinRpc;
use crate::storage::traits::StorageWrite;
use crate::types::Brc721Error;
use bitcoin::Block;

pub trait BlockParser<T: StorageWrite> {
    fn parse_block<R: BitcoinRpc>(
        &self,
        tx: &T,
        block: &Block,
        height: u64,
        rpc: &R,
    ) -> Result<(), Brc721Error>;
}
