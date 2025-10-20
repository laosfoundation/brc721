use bitcoin::{Block, BlockHash};
use bitcoincore_rpc::{Client, Error as RpcError, RpcApi};

pub trait BitcoinRpc {
    fn get_block_count(&self) -> Result<u64, RpcError>;
    fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError>;
    fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError>;
    fn wait_for_new_block(&self, timeout: u64) -> Result<(), RpcError>;
}

impl BitcoinRpc for Client {
    fn get_block_count(&self) -> Result<u64, RpcError> {
        RpcApi::get_block_count(self)
    }
    fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError> {
        RpcApi::get_block_hash(self, height)
    }
    fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError> {
        RpcApi::get_block(self, hash)
    }
    fn wait_for_new_block(&self, timeout: u64) -> Result<(), RpcError> {
        RpcApi::wait_for_new_block(self, timeout).map(|_| ())
    }
}

pub trait BlockScanner {
    fn next_blocks(&mut self) -> Result<&[(u64, Block)], RpcError>;
}

pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 60_000;

pub mod fetcher;
pub mod p2p;
pub mod rpc;

pub use fetcher::P2PFetcher;
pub use p2p::P2pScanner;
pub use rpc::RpcScanner;
