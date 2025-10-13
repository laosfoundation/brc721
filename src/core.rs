use crate::scanner::Scanner;
use crate::storage::Storage;
use std::sync::Arc;

pub struct Core<C: crate::scanner::BitcoinRpc> {
    storage: Arc<dyn Storage + Send + Sync>,
    scanner: Scanner<C>,
}

impl<C: crate::scanner::BitcoinRpc> Core<C> {
    pub fn new(storage: Arc<dyn Storage + Send + Sync>, scanner: Scanner<C>) -> Self {
        Self { storage, scanner }
    }

    pub fn run(mut self) -> ! {
        loop {
            match self.scanner.next_blocks() {
                Ok(blocks) => {
                    if let Some((height, block)) = blocks.last() {
                        let hash_str = block.block_hash().to_string();
                        if let Err(e) = self.storage.save_last(*height, &hash_str) {
                            eprintln!("storage error: {}", e);
                            std::process::exit(1);
                        }
                        println!("last block: height={}, hash={}", height, block.block_hash());
                    }
                }
                Err(e) => {
                    eprintln!("scanner error: {}", e);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
    }
}
