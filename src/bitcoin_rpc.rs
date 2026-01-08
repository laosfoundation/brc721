use bitcoin::{Block, BlockHash, Transaction, Txid};
use bitcoincore_rpc::{Error as RpcError, RpcApi};

pub trait BitcoinRpc {
    fn get_block_count(&self) -> Result<u64, RpcError>;
    fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError>;
    fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError>;
    fn get_raw_transaction(&self, txid: &Txid) -> Result<Transaction, RpcError>;
    fn wait_for_new_block(&self, timeout: u64) -> Result<(), RpcError>;
}

impl<T: RpcApi> BitcoinRpc for T {
    fn get_block_count(&self) -> Result<u64, RpcError> {
        RpcApi::get_block_count(self)
    }
    fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError> {
        RpcApi::get_block_hash(self, height)
    }
    fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError> {
        RpcApi::get_block(self, hash)
    }
    fn get_raw_transaction(&self, txid: &Txid) -> Result<Transaction, RpcError> {
        RpcApi::get_raw_transaction(self, txid, None)
    }
    fn wait_for_new_block(&self, timeout: u64) -> Result<(), RpcError> {
        RpcApi::wait_for_new_block(self, timeout).map(|_| ())
    }
}

