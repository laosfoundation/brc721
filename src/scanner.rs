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

const DEFAULT_WAIT_TIMEOUT_MS: u64 = 60_000;

pub struct Scanner<C: BitcoinRpc> {
    client: C,
    confirmations: u64,
    debug: bool,
    current_height: u64,
    wait_timeout_ms: u64,
}

impl<C: BitcoinRpc> Scanner<C> {
    pub fn new(client: C, confirmations: u64, debug: bool) -> Self {
        Self {
            client,
            confirmations,
            debug,
            current_height: 0,
            wait_timeout_ms: DEFAULT_WAIT_TIMEOUT_MS,
        }
    }

    pub fn start_from(&mut self, height: u64) {
        self.current_height = height;
    }

    pub fn next_blocks(&mut self, max: usize) -> Result<Vec<(u64, Block, BlockHash)>, RpcError> {
        if max == 0 {
            return Ok(Vec::new());
        }
        loop {
            let batch = self.collect_ready_blocks(max)?;
            if !batch.is_empty() {
                return Ok(batch);
            }
            self.client.wait_for_new_block(self.wait_timeout_ms)?;
        }
    }

    fn collect_ready_blocks(
        &mut self,
        max: usize,
    ) -> Result<Vec<(u64, Block, BlockHash)>, RpcError> {
        let mut out = Vec::with_capacity(max);
        for _ in 0..max {
            match self.next_ready_block()? {
                Some((height, block, hash_str)) => {
                    if self.debug {
                        println!("🧱 {} {} ({} txs)", height, hash_str, block.txdata.len());
                    }
                    out.push((height, block, hash_str));
                }
                None => break,
            }
        }
        Ok(out)
    }

    fn next_ready_block(&mut self) -> Result<Option<(u64, Block, BlockHash)>, RpcError> {
        let tip = self.client.get_block_count()?;
        if tip < self.confirmations {
            return Ok(None);
        }
        let target = tip.saturating_sub(self.confirmations);
        if self.current_height > target {
            return Ok(None);
        }
        let height = self.current_height;
        let hash = self.client.get_block_hash(height)?;
        let block = self.client.get_block(&hash)?;
        self.current_height += 1;
        Ok(Some((height, block, hash)))
    }
}
