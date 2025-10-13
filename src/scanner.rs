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

impl<'a, T: BitcoinRpc> BitcoinRpc for &'a T {
    fn get_block_count(&self) -> Result<u64, RpcError> {
        (*self).get_block_count()
    }
    fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError> {
        (*self).get_block_hash(height)
    }
    fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError> {
        (*self).get_block(hash)
    }
    fn wait_for_new_block(&self, timeout: u64) -> Result<(), RpcError> {
        (*self).wait_for_new_block(timeout)
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
    use bitcoin::{block::Header, block::Version, BlockHash, CompactTarget, TxMerkleNode};
    use std::cell::RefCell;

    fn make_block(prev: BlockHash, time: u32) -> Block {
        let header = Header {
            version: Version::TWO,
            prev_blockhash: prev,
            merkle_root: TxMerkleNode::all_zeros(),
            time,
            bits: CompactTarget::from_consensus(0),
            nonce: time,
        };
        Block {
            header,
            txdata: vec![],
        }
    }

    struct MockRpc {
        blocks: RefCell<Vec<Block>>,
    }

    impl MockRpc {
        fn new(len: usize) -> Self {
            let mut blocks = Vec::new();
            let mut prev = BlockHash::all_zeros();
            for i in 0..len as u32 {
                let b = make_block(prev, i);
                prev = b.header.block_hash();
                blocks.push(b);
            }
            Self {
                blocks: RefCell::new(blocks),
            }
        }
        fn append_block(&self) {
            let prev = self.blocks.borrow().last().unwrap().header.block_hash();
            let next_time = self.blocks.borrow().len() as u32;
            let b = make_block(prev, next_time);
            self.blocks.borrow_mut().push(b);
        }
    }

    impl BitcoinRpc for MockRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            let tip = self.blocks.borrow().len().saturating_sub(1) as u64;
            Ok(tip)
        }
        fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError> {
            Ok(self.blocks.borrow()[height as usize].header.block_hash())
        }
        fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError> {
            let b = self
                .blocks
                .borrow()
                .iter()
                .find(|b| b.header.block_hash() == *hash)
                .unwrap()
                .clone();
            Ok(b)
        }
        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            Ok(())
        }
    }

    fn drain_ready_heights(scanner: &mut Scanner<&MockRpc>, batch_size: usize) -> Vec<u64> {
        let mut heights = Vec::new();
        loop {
            let batch = scanner.collect_ready_blocks(batch_size).unwrap();
            if batch.is_empty() {
                break;
            }
            heights.extend(batch.into_iter().map(|(height, _, _)| height));
        }
        heights
    }

    #[test]
    fn processes_ready_blocks_respecting_confirmations() {
        let rpc = MockRpc::new(5);
        let mut scanner = Scanner::new(&rpc, 2, false);
        let heights = drain_ready_heights(&mut scanner, 1);
        assert_eq!(heights, vec![0, 1, 2]);
    }

    #[test]
    fn returns_empty_when_tip_less_than_confirmations() {
        let rpc = MockRpc::new(2);
        let mut scanner = Scanner::new(&rpc, 3, false);
        assert!(scanner.collect_ready_blocks(1).unwrap().is_empty());
        assert_eq!(scanner.current_height, 0);
    }

    #[test]
    fn returns_empty_when_max_is_zero() {
        let rpc = MockRpc::new(3);
        let mut scanner = Scanner::new(&rpc, 1, false);
        assert!(scanner.next_blocks(0).unwrap().is_empty());
        assert_eq!(scanner.current_height, 0);
    }

    #[test]
    fn respects_explicit_start_height() {
        let rpc = MockRpc::new(6);
        let mut scanner = Scanner::new(&rpc, 1, false);
        scanner.start_from(2);
        let heights = drain_ready_heights(&mut scanner, 3);
        assert_eq!(heights, vec![2, 3, 4]);
    }

    #[test]
    fn stops_when_current_height_exceeds_ready_target() {
        let rpc = MockRpc::new(4);
        let mut scanner = Scanner::new(&rpc, 1, false);
        scanner.start_from(5);
        assert!(scanner.collect_ready_blocks(2).unwrap().is_empty());
        assert_eq!(scanner.current_height, 5);
    }

    #[test]
    fn picks_up_new_blocks_after_chain_extends() {
        let rpc = MockRpc::new(4);
        let mut scanner = Scanner::new(&rpc, 1, false);
        let heights = drain_ready_heights(&mut scanner, 2);
        assert_eq!(heights, vec![0, 1, 2]);
        rpc.append_block();
        let next_batch = scanner.next_blocks(1).unwrap();
        assert_eq!(next_batch.first().unwrap().0, 3);
    }
}
