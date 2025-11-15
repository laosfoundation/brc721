use crate::parser::Parser;
use crate::scanner::Scanner;
use crate::storage::Storage;
use std::sync::Arc;

pub struct Core<C: crate::scanner::BitcoinRpc> {
    storage: Arc<dyn Storage + Send + Sync>,
    scanner: Scanner<C>,
    parser: Parser,
}

impl<C: crate::scanner::BitcoinRpc> Core<C> {
    pub fn new(
        storage: Arc<dyn Storage + Send + Sync>,
        scanner: Scanner<C>,
        parser: Parser,
    ) -> Self {
        Self {
            storage,
            scanner,
            parser,
        }
    }

    pub fn run(&mut self, shutdown: tokio_util::sync::CancellationToken) {
        loop {
            if shutdown.is_cancelled() {
                log::info!("🛑 Core shutdown requested");
                break;
            }
            match self
                .scanner
                .next_blocks_with_shutdown(&shutdown)
            {
                Ok(blocks) => {
                    for (height, block) in blocks {
                        log::info!("🧱 block={} 🧾 hash={}", height, block.block_hash());
                        if let Err(e) = self.parser.parse_block(block, *height, self.storage.as_ref()) {
                            log::error!(
                                "parsing error of block {} at height {}: {}",
                                block.block_hash(),
                                height,
                                e
                            );
                        }
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
        log::info!("👋 Core loop exited");
    }
}
