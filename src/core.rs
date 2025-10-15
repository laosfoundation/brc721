use crate::parser::Parser;
use crate::scanner::Scanner;
use crate::storage::Storage;
use std::sync::Arc;

pub struct Core<C: crate::scanner::BitcoinRpc> {
    storage: Arc<dyn Storage + Send + Sync>,
    scanner: Scanner<C>,
    parser: Arc<dyn Parser + Send + Sync>,
}

impl<C: crate::scanner::BitcoinRpc> Core<C> {
    pub fn new(
        storage: Arc<dyn Storage + Send + Sync>,
        scanner: Scanner<C>,
        parser: Arc<dyn Parser + Send + Sync>,
    ) -> Self {
        Self {
            storage,
            scanner,
            parser,
        }
    }

    pub fn run(mut self) -> ! {
        loop {
            match self.scanner.next_blocks() {
                Ok(blocks) => {
                    for (height, block) in blocks {
                        log::info!("block: height={}, hash={}", height, block.block_hash());
                        self.parser.parse_block(*height, block);
                        if let Err(e) = self
                            .storage
                            .save_last(*height, &block.block_hash().to_string())
                        {
                            log::error!(
                                "storage error saving block {} at height {}: {}",
                                block.block_hash(),
                                height,
                                e
                            );
                        }
                    }
                }
                Err(e) => {
                    log::error!("scanner error: {}", e);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
    }
}
