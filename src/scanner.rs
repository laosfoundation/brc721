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
    wait_timeout_ms: u64,
    max: usize,
    out: Vec<(u64, Block, BlockHash)>,
}

impl<C: BitcoinRpc> Scanner<C> {
    pub fn new(client: C, confirmations: u64, max: usize) -> Self {
        Self {
            client,
            confirmations,
            current_height: 0,
            wait_timeout_ms: DEFAULT_WAIT_TIMEOUT_MS,
            max,
            out: Vec::with_capacity(max),
        }
    }

    pub fn start_from(&mut self, height: u64) {
        self.current_height = height;
    }

    pub fn next_blocks(&mut self) -> Result<&[(u64, Block, BlockHash)], RpcError> {
        if self.max == 0 {
            self.out.clear();
            return Ok(self.out.as_slice());
        }
        loop {
            self.collect_ready_blocks()?;
            if !self.out.is_empty() {
                return Ok(self.out.as_slice());
            }
            self.client.wait_for_new_block(self.wait_timeout_ms)?;
        }
    }

    fn collect_ready_blocks(&mut self) -> Result<&[(u64, Block, BlockHash)], RpcError> {
        self.out.clear();
        for _ in 0..self.max {
            match self.next_ready_block()? {
                Some((height, block, hash_str)) => {
                    self.out.push((height, block, hash_str));
                }
                None => break,
            }
        }
        Ok(self.out.as_slice())
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
        let mut scanner = Scanner::new(mock, 0, 0);
        let v = scanner.next_blocks().unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn collect_ready_blocks_empty_when_tip_less_than_confirmations() {
        let blocks = MockRpc::make_chain(1);
        let mock = MockRpc::new(blocks, 0);
        let mut scanner = Scanner::new(mock, 1, 5);
        let v = scanner.collect_ready_blocks().unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn next_ready_block_returns_some_and_increments() {
        let blocks = MockRpc::make_chain(1);
        let mock = MockRpc::new(blocks.clone(), 1);
        let mut scanner = Scanner::new(mock, 1, 1);
        let r = scanner.next_ready_block().unwrap();
        assert!(r.is_some());
        assert_eq!(scanner.current_height, 1);
    }

    #[test]
    fn next_blocks_returns_batch_up_to_max() {
        let blocks = MockRpc::make_chain(4);
        let mock = MockRpc::new(blocks, 4);
        let mut scanner = Scanner::new(mock, 1, 2);
        let a = scanner.next_blocks().unwrap();
        assert_eq!(a.len(), 2);
        let b = scanner.next_blocks().unwrap();
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
        let mut scanner = Scanner::new(mock, 1, 1);
        let v = scanner.next_blocks().unwrap();
        assert_eq!(v.len(), 1);
    }

    use std::rc::Rc;

    struct Inner {
        tip: Cell<u64>,
        waits: Cell<u64>,
    }

    #[derive(Clone)]
    struct WaitCounterRpc {
        blocks: Vec<(BlockHash, Block)>,
        inner: Rc<Inner>,
        inc: u64,
    }

    impl WaitCounterRpc {
        fn new(blocks: Vec<(BlockHash, Block)>, tip: u64, inc: u64) -> Self {
            Self {
                blocks,
                inner: Rc::new(Inner {
                    tip: Cell::new(tip),
                    waits: Cell::new(0),
                }),
                inc,
            }
        }
        fn waits(&self) -> u64 {
            self.inner.waits.get()
        }
    }

    impl BitcoinRpc for WaitCounterRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            Ok(self.inner.tip.get())
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
            self.inner.waits.set(self.inner.waits.get() + 1);
            self.inner.tip.set(self.inner.tip.get() + self.inc);
            Ok(())
        }
    }

    #[test]
    fn start_from_nonzero_respects_confirmations() {
        let blocks = MockRpc::make_chain(6);
        let mock = MockRpc::new(blocks.clone(), 5);
        let mut scanner = Scanner::new(mock, 2, 10);
        scanner.start_from(2);
        let batch = scanner.next_blocks().unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].0, 2);
        assert_eq!(batch[1].0, 3);
        assert_eq!(batch[0].2, blocks[2].0);
        assert_eq!(batch[1].2, blocks[3].0);
    }

    #[test]
    fn next_ready_block_none_when_current_height_above_target() {
        let blocks = MockRpc::make_chain(3);
        let mock = MockRpc::new(blocks, 2);
        let mut scanner = Scanner::new(mock, 0, 1);
        scanner.start_from(3);
        let r = scanner.next_ready_block().unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn next_blocks_does_not_wait_when_blocks_ready() {
        let blocks = MockRpc::make_chain(3);
        let rpc = WaitCounterRpc::new(blocks, 2, 1);
        let rpc_view = rpc.clone();
        let mut scanner = Scanner::new(rpc, 0, 1);
        let v = scanner.next_blocks().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(rpc_view.waits(), 0);
    }

    #[test]
    fn next_blocks_returns_early_when_any_ready_even_if_max_not_filled() {
        let blocks = MockRpc::make_chain(4);
        let rpc = WaitCounterRpc::new(blocks, 0, 1);
        let rpc_view = rpc.clone();
        let mut scanner = Scanner::new(rpc, 2, 5);
        let v = scanner.next_blocks().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(rpc_view.waits(), 2);
    }

    #[test]
    fn collect_ready_blocks_respects_max_and_drains() {
        let blocks = MockRpc::make_chain(6);
        let mock = MockRpc::new(blocks, 5);
        let mut scanner = Scanner::new(mock, 1, 3);
        let a = scanner.collect_ready_blocks().unwrap();
        assert_eq!(a.len(), 3);
        assert_eq!(a[0].0, 0);
        assert_eq!(a[1].0, 1);
        assert_eq!(a[2].0, 2);
        let b = scanner.collect_ready_blocks().unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].0, 3);
        assert_eq!(b[1].0, 4);
        let c = scanner.collect_ready_blocks().unwrap();
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn returned_hash_matches_block_hash() {
        let blocks = MockRpc::make_chain(3);
        let mock = MockRpc::new(blocks.clone(), 2);
        let mut scanner = Scanner::new(mock, 0, 5);
        scanner.start_from(1);
        let batch = scanner.collect_ready_blocks().unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].0, 1);
        assert_eq!(batch[0].2, blocks[1].0);
        assert_eq!(batch[1].0, 2);
        assert_eq!(batch[1].2, blocks[2].0);
    }
}
