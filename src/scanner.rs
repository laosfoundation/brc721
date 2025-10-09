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

    pub fn start_from(&mut self, height: u64) {
        self.current_height = height;
    }

    pub fn step(&mut self) -> Result<Option<(u64, Block, String)>, RpcError> {
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
        Ok(Some((height, block, hash.to_string())))
    }

    pub fn run<F>(&mut self, mut on_block: F) -> !
    where
        F: FnMut(u64, &Block, &str),
    {
        loop {
            loop {
                match self.step() {
                    Ok(Some((height, block, hash_str))) => {
                        if self.debug {
                            println!("🧱 {} {} ({} txs)", height, hash_str, block.txdata.len());
                        }
                        on_block(height, &block, &hash_str);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("scanner step error: {}", e);
                        thread::sleep(Duration::from_millis(500));
                        break;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{block::Header, block::Version, BlockHash, CompactTarget, TxMerkleNode};
    use bitcoin::hashes::Hash;
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
        Block { header, txdata: vec![] }
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
            Self { blocks: RefCell::new(blocks) }
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
        fn wait_for_new_block(&self, _timeout_secs: u64) -> Result<(), RpcError> {
            Ok(())
        }
    }

    #[test]
    fn step_processes_up_to_tip_minus_confirmations() {
        let rpc = MockRpc::new(5);
        let mut scanner = Scanner::new(&rpc, 2, false);
        let mut heights = Vec::new();
        loop {
            match scanner.step().unwrap() {
                Some((h, _b, _hs)) => heights.push(h),
                None => break,
            }
        }
        assert_eq!(heights, vec![0, 1, 2]);
    }

    #[test]
    fn step_no_processing_when_tip_less_than_confirmations() {
        let rpc = MockRpc::new(2);
        let mut scanner = Scanner::new(&rpc, 3, false);
        let r = scanner.step().unwrap();
        assert!(r.is_none());
        assert_eq!(scanner.current_height, 0);
    }

    #[test]
    fn step_picks_up_new_blocks_after_append() {
        let rpc = MockRpc::new(4);
        let mut scanner = Scanner::new(&rpc, 1, false);
        let mut heights = Vec::new();
        loop {
            match scanner.step().unwrap() {
                Some((h, _b, _hs)) => heights.push(h),
                None => break,
            }
        }
        assert_eq!(heights, vec![0, 1, 2]);
        rpc.append_block();
        let r = scanner.step().unwrap();
        assert_eq!(r.unwrap().0, 3);
    }
}
