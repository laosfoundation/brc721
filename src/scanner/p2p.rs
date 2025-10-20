use super::{BitcoinRpc, BlockScanner, DEFAULT_WAIT_TIMEOUT_MS};
use crate::scanner::P2PFetcher;
use bitcoin::Block;
use bitcoincore_rpc::Error as RpcError;

pub struct P2pScanner<C: BitcoinRpc> {
    client: C,
    p2p: P2PFetcher,
    confirmations: u64,
    current_height: u64,
    out: Vec<(u64, Block)>,
}

impl<C: BitcoinRpc> P2pScanner<C> {
    pub fn new(client: C, p2p: P2PFetcher) -> Self {
        Self {
            client,
            p2p,
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

    fn collect_ready_blocks(&mut self) -> Result<&[(u64, Block)], RpcError> {
        self.out.clear();
        let tip = self.client.get_block_count()?;
        if tip < self.confirmations {
            return Ok(self.out.as_slice());
        }
        let target = tip.saturating_sub(self.confirmations);
        if self.current_height > target {
            return Ok(self.out.as_slice());
        }
        let avail = (target - self.current_height + 1) as usize;
        let to_fetch = self.out.capacity().min(avail);
        if to_fetch == 0 {
            return Ok(self.out.as_slice());
        }
        let start = self.current_height;
        let mut heights = Vec::with_capacity(to_fetch);
        for i in 0..to_fetch {
            heights.push(start + i as u64);
        }
        log::debug!("collecting {} blocks from height {}..=", to_fetch, start);
        let mut hashes = Vec::with_capacity(to_fetch);
        for h in &heights {
            hashes.push(self.client.get_block_hash(*h)?);
        }
        log::debug!("attempt p2p fetch for {} blocks", hashes.len());
        match self.p2p.fetch_blocks(&hashes) {
            Ok(blocks) => {
                log::info!("p2p fetched {} blocks", blocks.len());
                for (i, b) in blocks.into_iter().enumerate() {
                    self.out.push((heights[i], b));
                }
                self.current_height += to_fetch as u64;
            }
            Err(e) => {
                log::warn!("p2p fetch failed: {}", e);
            }
        }
        Ok(self.out.as_slice())
    }
}

impl<C: BitcoinRpc> BlockScanner for P2pScanner<C> {
    fn next_blocks(&mut self) -> Result<&[(u64, Block)], RpcError> {
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
}
