use super::{BitcoinRpc, BlockScanner, DEFAULT_WAIT_TIMEOUT_MS};
use bitcoin::Block;
use bitcoincore_rpc::Error as RpcError;

pub struct RpcScanner<C: BitcoinRpc> {
    client: C,
    confirmations: u64,
    current_height: u64,
    out: Vec<(u64, Block)>,
}

impl<C: BitcoinRpc> RpcScanner<C> {
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
        log::debug!("fetching {} blocks via RPC", hashes.len());
        for (i, h) in hashes.iter().enumerate() {
            let b = self.client.get_block(h)?;
            self.out.push((heights[i], b));
        }
        self.current_height += to_fetch as u64;
        Ok(self.out.as_slice())
    }
}

impl<C: BitcoinRpc> BlockScanner for RpcScanner<C> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use bitcoin::BlockHash;
    use bitcoin::{
        absolute::LockTime, block::Header, block::Version, transaction::Version as TxVersion,
        Amount, CompactTarget, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxMerkleNode,
        TxOut,
    };
    use bitcoincore_rpc::Error as RpcError;

    struct MockRpc {
        tip: u64,
        blocks: std::collections::HashMap<u64, (BlockHash, Block)>,
    }

    impl MockRpc {
        fn new(tip: u64) -> Self {
            Self {
                tip,
                blocks: std::collections::HashMap::new(),
            }
        }
        fn with_block(mut self, height: u64, hash: BlockHash, block: Block) -> Self {
            self.blocks.insert(height, (hash, block));
            self
        }
    }

    impl super::BitcoinRpc for MockRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            Ok(self.tip)
        }
        fn get_block_hash(&self, height: u64) -> Result<BlockHash, RpcError> {
            Ok(self.blocks.get(&height).unwrap().0)
        }
        fn get_block(&self, hash: &BlockHash) -> Result<Block, RpcError> {
            let (_h, b) = self.blocks.values().find(|(hh, _)| hh == hash).unwrap();
            Ok(b.clone())
        }
        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            Ok(())
        }
    }

    fn dummy_block(prev: BlockHash) -> Block {
        let header = Header {
            version: Version::TWO,
            prev_blockhash: prev,
            merkle_root: TxMerkleNode::all_zeros(),
            time: 0,
            bits: CompactTarget::from_consensus(0),
            nonce: 0,
        };
        let txin = TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: bitcoin::Witness::default(),
        };
        let txout = TxOut {
            value: Amount::from_sat(0),
            script_pubkey: ScriptBuf::new(),
        };
        let tx = Transaction {
            version: TxVersion::TWO,
            lock_time: LockTime::ZERO,
            input: vec![txin],
            output: vec![txout],
        };
        Block {
            header,
            txdata: vec![tx],
        }
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

        let mut scanner = RpcScanner::new(rpc)
            .with_confirmations(0)
            .with_capacity(2)
            .with_start_from(start);

        let out = scanner.next_blocks().unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, start);
        assert_eq!(out[1].0, start + 1);
    }
}
