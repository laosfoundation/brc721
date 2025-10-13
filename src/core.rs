use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bitcoin::{Block, BlockHash};

use crate::parser;
use crate::scanner::Scanner;
use crate::storage::Storage;

pub struct Core<C: crate::scanner::BitcoinRpc> {
    storage: Arc<dyn Storage + Send + Sync>,
    scanner: Scanner<C>,
    debug: bool,
    batch_size: usize,
}

impl<C: crate::scanner::BitcoinRpc> Core<C> {
    pub fn new(storage: Arc<dyn Storage + Send + Sync>, scanner: Scanner<C>, debug: bool, batch_size: usize) -> Self {
        Self { storage, scanner, debug, batch_size }
    }

    fn is_orphan(prev: &crate::storage::Block, block: &Block) -> bool {
        block.header.prev_blockhash.to_string() != prev.hash
    }

    fn format_block_scripts(block: &Block) -> String {
        let mut out = String::new();
        for tx in &block.txdata {
            let txid = tx.compute_txid();
            out.push_str(&format!("tx {}:\n", txid));
            for (i, input) in tx.input.iter().enumerate() {
                let script_sig_hex = hex::encode(input.script_sig.as_bytes());
                out.push_str(&format!("  vin[{}] scriptSig: {}\n", i, script_sig_hex));
                if !input.witness.is_empty() {
                    let wit_items: Vec<String> = input.witness.iter().map(hex::encode).collect();
                    out.push_str(&format!(
                        "  vin[{}] witness: [{}]\n",
                        i,
                        wit_items.join(", ")
                    ));
                }
            }
            for (j, output) in tx.output.iter().enumerate() {
                let script_pubkey_hex = hex::encode(output.script_pubkey.as_bytes());
                out.push_str(&format!(
                    "  vout[{}] scriptPubKey: {}\n",
                    j, script_pubkey_hex
                ));
            }
        }
        out
    }

    fn process_block(storage: Arc<dyn Storage + Send + Sync>, block: &Block, debug: bool, height: u64, block_hash: &BlockHash) {
        if debug {
            let s = Self::format_block_scripts(block);
            print!("{}", s);
        } else {
            parser::parse_with_repo(storage.as_ref(), height, block, block_hash)
        }
    }

    pub fn run(mut self) -> ! {
        let batch_size = self.batch_size;
        let storage2 = self.storage.clone();

        if batch_size <= 1 {
            loop {
                let blocks = match self.scanner.next_blocks() {
                    Ok(blocks) => blocks,
                    Err(e) => {
                        eprintln!("scanner next_blocks error: {}", e);
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                };
                let mut last_processed: Option<(u64, String)> = None;
                for (height, block, hash) in blocks.iter().map(|(h, b, hs)| (*h, b, hs)) {
                    if let Ok(Some(prev)) = storage2.load_last() {
                        if Self::is_orphan(&prev, &block) {
                            eprintln!(
                                "error: detected orphan branch at height {}: parent {} != last processed {}",
                                height, block.header.prev_blockhash, prev.hash
                            );
                            std::process::exit(1);
                        }
                    }
                    Self::process_block(storage2.clone(), block, self.debug, height, hash);
                    let hash_str = hash.to_string();
                    if let Err(e) = storage2.save_last(height, &hash_str) {
                        eprintln!(
                            "warning: failed to save last block {} ({}): {}",
                            height, hash_str, e
                        );
                    }
                    last_processed = Some((height, hash_str));
                }
                if let Some((h, hs)) = last_processed.take() {
                    println!("last processed: {} {}", h, hs);
                }
            }
        } else {
            loop {
                let items = match self.scanner.next_blocks() {
                    Ok(items) => items,
                    Err(e) => {
                        eprintln!("scanner next_blocks error: {}", e);
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                };
                if let Ok(Some(prev)) = storage2.load_last() {
                    let mut expected = prev.hash.clone();
                    for (h, b, hs) in items.iter() {
                        if b.header.prev_blockhash.to_string() != expected {
                            eprintln!(
                                "error: detected orphan branch at height {}: parent {} != last processed {}",
                                h, b.header.prev_blockhash, expected
                            );
                            std::process::exit(1);
                        }
                        expected = hs.to_string();
                    }
                }
                let refs: Vec<(u64, &Block, &BlockHash)> =
                    items.iter().map(|(h, b, hs)| (*h, b, hs)).collect();
                parser::parse_blocks_batch(storage2.as_ref(), &refs);
                if let Some((last_h, _b, last_hash)) = items.last() {
                    let last_hash_str = last_hash.to_string();
                    if let Err(e) = storage2.save_last(*last_h, &last_hash_str) {
                        eprintln!(
                            "warning: failed to save last block {} ({}): {}",
                            last_h, last_hash_str, e
                        );
                    }
                    println!("last processed: {} {}", last_h, last_hash_str);
                }
            }
        }
    }
}
