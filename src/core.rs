use crate::parser::Parser;
use crate::scanner::Scanner;
use crate::storage::Storage;
use bitcoin::Block;
use std::sync::Arc;
use std::time::Duration;

const SCANNER_BACKOFF: Duration = Duration::from_secs(1);

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
