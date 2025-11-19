use crate::parser::BlockParse;
use crate::scanner::Scanner;
use crate::storage::Storage;
use bitcoin::Block;
use std::sync::Arc;
use std::time::Duration;

const SCANNER_BACKOFF: Duration = Duration::from_secs(1);

pub struct Core<C: crate::scanner::BitcoinRpc, P: BlockParse> {
    storage: Arc<dyn Storage + Send + Sync>,
    scanner: Scanner<C>,
    parser: P,
}

impl<C: crate::scanner::BitcoinRpc, P: BlockParse> Core<C, P> {
    pub fn new(storage: Arc<dyn Storage + Send + Sync>, scanner: Scanner<C>, parser: P) -> Self {
        Self {
            storage,
            scanner,
            parser,
        }
    }

    /// Main loop: keep stepping until shutdown is requested.
    pub fn run(&mut self, shutdown: tokio_util::sync::CancellationToken) {
        while !shutdown.is_cancelled() {
            self.step(&shutdown);
        }
        log::info!("👋 Core loop exited");
    }

    /// One iteration: ask the scanner for blocks, process them, or back off on error.
    pub fn step(&mut self, shutdown: &tokio_util::sync::CancellationToken) {
        match self.scanner.next_blocks_with_shutdown(shutdown) {
            Ok(blocks) => {
                for (height, block) in blocks {
                    self.process_block(height, &block);
                }
            }
            Err(e) => {
                log::error!("scanner error: {}", e);
                std::thread::sleep(SCANNER_BACKOFF);
            }
        }
    }

    fn process_block(&self, height: u64, block: &Block) {
        let hash = block.block_hash();
        log::info!("🧱 block={} 🧾 hash={}", height, hash);

        if let Err(e) = self.parser.parse_block(block, height) {
            log::error!(
                "parsing error of block {} at height {}: {}",
                hash,
                height,
                e
            );
            return;
        }

        if let Err(e) = self.storage.save_last(height, &hash.to_string()) {
            log::error!(
                "storage error saving block {} at height {}: {}",
                hash,
                height,
                e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::storage::traits::{Block as StorageBlock, CollectionKey};
    use crate::storage::Storage;
    use anyhow::anyhow;
    use bitcoin::blockdata::constants::genesis_block;
    use bitcoin::Network;
    use bitcoincore_rpc::Error as RpcError;
    use ethereum_types::H160;
    use std::sync::Arc;
    use std::sync::Mutex;

    struct DummyStorage {
        last_height: Mutex<Option<u64>>,
        last_hash: Mutex<Option<String>>,
        fail: bool,
    }

    impl DummyStorage {
        fn new(fail: bool) -> Self {
            Self {
                last_height: Mutex::new(None),
                last_hash: Mutex::new(None),
                fail,
            }
        }
    }

    impl Storage for DummyStorage {
        fn load_last(&self) -> anyhow::Result<Option<StorageBlock>> {
            Ok(None)
        }

        fn save_last(&self, height: u64, hash: &str) -> anyhow::Result<()> {
            if self.fail {
                return Err(anyhow!("fail"));
            }
            *self.last_height.lock().unwrap() = Some(height);
            *self.last_hash.lock().unwrap() = Some(hash.to_string());
            Ok(())
        }

        fn save_collection(
            &self,
            _key: CollectionKey,
            _evm_collection_address: H160,
            _rebaseable: bool,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn list_collections(&self) -> anyhow::Result<Vec<(CollectionKey, String, bool)>> {
            Ok(Vec::new())
        }
    }

    #[derive(Clone)]
    struct DummyRpc;

    impl crate::scanner::BitcoinRpc for DummyRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            unimplemented!()
        }

        fn get_block_hash(&self, _height: u64) -> Result<bitcoin::BlockHash, RpcError> {
            unimplemented!()
        }

        fn get_block(&self, _hash: &bitcoin::BlockHash) -> Result<bitcoin::Block, RpcError> {
            unimplemented!()
        }

        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            unimplemented!()
        }
    }

    fn empty_block() -> Block {
        genesis_block(Network::Regtest)
    }

    fn make_core(fail_storage: bool) -> (Arc<DummyStorage>, Core<DummyRpc, Parser>) {
        let inner = Arc::new(DummyStorage::new(fail_storage));
        let storage: Arc<dyn Storage + Send + Sync> = inner.clone();
        let parser = Parser::new(storage.clone());
        let rpc = DummyRpc;
        let scanner = Scanner::new(rpc);
        let core = Core::new(storage, scanner, parser);
        (inner, core)
    }

    #[test]
    fn process_block_success_saves_height_and_hash() {
        let (inner, core) = make_core(false);

        let block = empty_block();
        let height = 42;
        core.process_block(height, &block);

        assert_eq!(*inner.last_height.lock().unwrap(), Some(height));
        assert_eq!(
            *inner.last_hash.lock().unwrap(),
            Some(block.block_hash().to_string())
        );
    }

    #[test]
    fn process_block_storage_error_does_not_panic() {
        let (inner, core) = make_core(true);

        let block = empty_block();
        let height = 1;
        core.process_block(height, &block);

        assert_eq!(*inner.last_height.lock().unwrap(), None);
        assert_eq!(*inner.last_hash.lock().unwrap(), None);
    }
}
