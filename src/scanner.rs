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
    out: Vec<(u64, Block, BlockHash)>,
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

    pub fn next_blocks(&mut self) -> Result<&[(u64, Block, BlockHash)], RpcError> {
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

    fn collect_ready_blocks(&mut self) -> Result<&[(u64, Block, BlockHash)], RpcError> {
        self.out.clear();
        for _ in 0..self.out.capacity() {
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
    use bitcoin::{block::Header, block::Version, absolute::LockTime, transaction::Version as TxVersion, TxMerkleNode, CompactTarget, Transaction, TxIn, TxOut, Amount, Sequence, OutPoint, ScriptBuf};
    use bitcoin::hashes::Hash;
    use bitcoincore_rpc::Error as RpcError;

    struct MockRpc {
        tip: u64,
        blocks: std::collections::HashMap<u64, (BlockHash, Block)>,
    }

    impl MockRpc {
        fn new(tip: u64) -> Self {
            Self { tip, blocks: std::collections::HashMap::new() }
        }
        fn with_block(mut self, height: u64, hash: BlockHash, block: Block) -> Self {
            self.blocks.insert(height, (hash, block));
            self
        }
    }

    impl BitcoinRpc for MockRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> { Ok(self.tip) }
        fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError> { Ok(self.blocks.get(&height).unwrap().0) }
        fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError> {
            let (_h, b) = self.blocks.values().find(|(hh, _)| hh == hash).unwrap();
            Ok(b.clone())
        }
        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> { Ok(()) }
    }

    fn dummy_block(prev: BlockHash) -> Block {
        let header = Header { version: Version::TWO, prev_blockhash: prev, merkle_root: TxMerkleNode::all_zeros(), time: 0, bits: CompactTarget::from_consensus(0), nonce: 0 };
        let txin = TxIn { previous_output: OutPoint::null(), script_sig: ScriptBuf::new(), sequence: Sequence::MAX, witness: bitcoin::Witness::default() };
        let txout = TxOut { value: Amount::from_sat(0), script_pubkey: ScriptBuf::new() };
        let tx = Transaction { version: TxVersion::TWO, lock_time: LockTime::ZERO, input: vec![txin], output: vec![txout] };
        Block { header, txdata: vec![tx] }
    }

    #[test]
    fn starts_from_provided_start_height_when_no_state() {
        let start = 1000u64;
        let tip = 1005u64;
        let h0 = bitcoin::BlockHash::all_zeros();
        let b0 = dummy_block(h0);
        let h1 = b0.header.block_hash();
        let b1 = dummy_block(h1);
        let h2 = b1.header.block_hash();

        let rpc = MockRpc::new(tip)
            .with_block(start, h1, b0.clone())
            .with_block(start + 1, h2, b1.clone());

        let mut scanner = Scanner::new(rpc)
            .with_confirmations(0)
            .with_capacity(2)
            .with_start_from(start);

        let out = scanner.next_blocks().unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, start);
        assert_eq!(out[1].0, start + 1);
    }
}
