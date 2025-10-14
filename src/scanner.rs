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
    current_height: u64,
    out: Vec<(u64, Block)>,
}

impl<C: BitcoinRpc> Scanner<C> {
    pub fn new(client: C) -> Self {
        Self {
            client,
            confirmations: 0,
            current_height: 0,
            out: Vec::new(),
        }
    }

    pub fn with_confirmations(mut self, confirmations: u64) -> Self {
        self.confirmations = confirmations;
        self
    }

    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.out = Vec::with_capacity(capacity);
        self
    }

    pub fn with_start_from(mut self, height: u64) -> Self {
        self.current_height = height;
        self
    }

    pub fn next_blocks(&mut self) -> Result<&[(u64, Block)], RpcError> {
        if self.out.capacity() == 0 {
            self.out.clear();
            return Ok(self.out.as_slice());
        }
        loop {
            self.collect_ready_blocks()?;
            if !self.out.is_empty() {
                return Ok(self.out.as_slice());
            }
            self.client.wait_for_new_block(DEFAULT_WAIT_TIMEOUT_MS)?;
        }
    }

    fn collect_ready_blocks(&mut self) -> Result<&[(u64, Block)], RpcError> {
        self.out.clear();
        for _ in 0..self.out.capacity() {
            match self.next_ready_block()? {
                Some((height, block)) => {
                    self.out.push((height, block));
                }
                None => break,
            }
        }
        Ok(self.out.as_slice())
    }

    fn next_ready_block(&mut self) -> Result<Option<(u64, Block)>, RpcError> {
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
        Ok(Some((height, block)))
    }
}
