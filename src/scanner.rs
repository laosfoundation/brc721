use std::thread;
use std::time::Duration;

use bitcoin::{Block, BlockHash};
use bitcoincore_rpc::{Client, Error as RpcError, RpcApi};

pub trait BitcoinRpc {
    fn get_block_count(&self) -> Result<u64, RpcError>;
    fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError>;
    fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError>;
    fn wait_for_new_block(&self, timeout_secs: u64) -> Result<(), RpcError>;
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
    fn wait_for_new_block(&self, timeout_secs: u64) -> Result<(), RpcError> {
        RpcApi::wait_for_new_block(self, timeout_secs).map(|_| ())
    }
}

pub struct Scanner<'a, C: BitcoinRpc + ?Sized> {
    client: &'a C,
    confirmations: u64,
    debug: bool,
    current_height: u64,
}

impl<'a, C: BitcoinRpc + ?Sized> Scanner<'a, C> {
    pub fn new(client: &'a C, confirmations: u64, debug: bool) -> Self {
        Self { client, confirmations, debug, current_height: 0 }
    }

    pub fn run<F>(&mut self, mut on_block: F) -> !
    where
        F: FnMut(u64, &Block, &str),
    {
        loop {
            match self.client.get_block_count() {
                Ok(tip) => {
                    if tip >= self.confirmations {
                        let target = tip.saturating_sub(self.confirmations);
                        while self.current_height <= target {
                            match self.client.get_block_hash(self.current_height) {
                                Ok(hash) => match self.client.get_block(&hash) {
                                    Ok(block) => {
                                        if self.debug {
                                            println!(
                                                "🧱 {} {} ({} txs)",
                                                self.current_height,
                                                hash,
                                                block.txdata.len()
                                            );
                                        }
                                        on_block(self.current_height, &block, &hash.to_string());
                                        self.current_height += 1;
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "error get_block at height {}: {}",
                                            self.current_height, e
                                        );
                                        thread::sleep(Duration::from_millis(500));
                                    }
                                },
                                Err(e) => {
                                    eprintln!(
                                        "error get_block_hash at height {}: {}",
                                        self.current_height, e
                                    );
                                    thread::sleep(Duration::from_millis(500));
                                }
                            }
                        }
                    }

                    match self.client.wait_for_new_block(60) {
                        Ok(()) => {}
                        Err(_e) => {
                            thread::sleep(Duration::from_secs(1));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("error get_block_count: {}", e);
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }
}
