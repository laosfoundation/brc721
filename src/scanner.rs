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

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;

    #[derive(Clone)]
    struct MockRpc {
        tip: u64,
        blocks: Vec<(BlockHash, Block)>,
    }

    impl MockRpc {
        fn make_chain(n: usize) -> Vec<(BlockHash, Block)> {
            let mut out = Vec::with_capacity(n);
            let mut prev = BlockHash::all_zeros();
            for i in 0..n {
                let header = bitcoin::block::Header {
                    version: bitcoin::block::Version::TWO,
                    prev_blockhash: prev,
                    merkle_root: bitcoin::TxMerkleNode::all_zeros(),
                    time: i as u32,
                    bits: bitcoin::CompactTarget::from_consensus(0),
                    nonce: i as u32,
                };
                let block = Block {
                    header,
                    txdata: vec![],
                };
                let hash = block.header.block_hash();
                out.push((hash, block));
                prev = hash;
            }
            out
        }

        fn new(blocks: Vec<(BlockHash, Block)>, tip: u64) -> Self {
            Self { tip, blocks }
        }
    }

    impl BitcoinRpc for MockRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            Ok(self.tip)
        }
        fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError> {
            Ok(self.blocks[height as usize].0)
        }
        fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError> {
            let b = self
                .blocks
                .iter()
                .find(|(h, _)| h == hash)
                .unwrap()
                .1
                .clone();
            Ok(b)
        }
        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            Ok(())
        }
    }

    #[test]
    fn next_blocks_zero_returns_empty() {
        let blocks = MockRpc::make_chain(3);
        let mock = MockRpc::new(blocks, 2);
        let mut scanner = Scanner::new(mock, 0, false);
        let v = scanner.next_blocks(0).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn collect_ready_blocks_empty_when_tip_less_than_confirmations() {
        let blocks = MockRpc::make_chain(1);
        let mock = MockRpc::new(blocks, 0);
        let mut scanner = Scanner::new(mock, 1, false);
        let v = scanner.collect_ready_blocks(5).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn next_ready_block_returns_some_and_increments() {
        let blocks = MockRpc::make_chain(1);
        let mock = MockRpc::new(blocks.clone(), 1);
        let mut scanner = Scanner::new(mock, 1, false);
        let r = scanner.next_ready_block().unwrap();
        assert!(r.is_some());
        assert_eq!(scanner.current_height, 1);
    }

    #[test]
    fn next_blocks_returns_batch_up_to_max() {
        let blocks = MockRpc::make_chain(4);
        let mock = MockRpc::new(blocks, 4);
        let mut scanner = Scanner::new(mock, 1, false);
        let a = scanner.next_blocks(2).unwrap();
        assert_eq!(a.len(), 2);
        let b = scanner.next_blocks(10).unwrap();
        assert_eq!(b.len(), 2);
    }

    use std::cell::Cell;

    struct WaitRpc {
        blocks: Vec<(BlockHash, Block)>,
        tip: Cell<u64>,
    }

    impl BitcoinRpc for WaitRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            Ok(self.tip.get())
        }
        fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError> {
            Ok(self.blocks[height as usize].0)
        }
        fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError> {
            Ok(self
                .blocks
                .iter()
                .find(|(h, _)| h == hash)
                .unwrap()
                .1
                .clone())
        }
        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            self.tip.set(self.tip.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn next_blocks_waits_until_ready() {
        let blocks = MockRpc::make_chain(1);
        let mock = WaitRpc {
            blocks,
            tip: Cell::new(0),
        };
        let mut scanner = Scanner::new(mock, 1, false);
        let v = scanner.next_blocks(1).unwrap();
        assert_eq!(v.len(), 1);
    }
}
