use std::env;

use bitcoin::Block;
use bitcoincore_rpc::{Auth, Client};
use dotenvy::dotenv;
mod cli;
mod db;
mod parser;
mod scanner;
mod storage;
use crate::storage::Storage;
use std::sync::Arc;

fn is_orphan(prev: &storage::LastBlock, block: &Block) -> bool {
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

fn process_block(
    repo: Arc<dyn db::Repository + Send + Sync>,
    block: &Block,
    debug: bool,
    height: u64,
    block_hash_str: &str,
) {
    if debug {
        let s = format_block_scripts(block);
        print!("{}", s);
    } else {
        parser::parse_with_repo(repo.as_ref(), height, block, block_hash_str)
    }
}

fn main() {
    dotenv().ok();

    let cli = cli::parse();
    let debug = cli.debug;
    let confirmations = cli.confirmations;

    let rpc_url =
        env::var("BITCOIN_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8332".to_string());

    let auth = match (
        env::var("BITCOIN_RPC_USER").ok(),
        env::var("BITCOIN_RPC_PASS").ok(),
        env::var("BITCOIN_RPC_COOKIE").ok(),
    ) {
        (Some(user), Some(pass), _) => Auth::UserPass(user, pass),
        (_, _, Some(cookie_path)) => Auth::CookieFile(cookie_path.into()),
        _ => Auth::None,
    };

    let auth_mode = match (
        env::var("BITCOIN_RPC_USER").ok(),
        env::var("BITCOIN_RPC_PASS").ok(),
        env::var("BITCOIN_RPC_COOKIE").ok(),
    ) {
        (Some(_), Some(_), _) => "user/pass",
        (_, _, Some(_)) => "cookie",
        _ => "none",
    };

    println!("🚀 Starting brc721");
    println!("🔗 RPC URL: {}", rpc_url);
    println!("🔐 Auth: {}", auth_mode);
    println!("🛠️ Debug: {}", if debug { "on" } else { "off" });
    println!("🧮 Confirmations: {}", confirmations);

    let client = Client::new(&rpc_url, auth).expect("failed to create RPC client");

    let default_db = "./.brc721/brc721.sqlite".to_string();
    let db_path = env::var("BRC721_DB_PATH").unwrap_or(default_db);

    let (store, repo): (
        Arc<dyn Storage + Send + Sync>,
        Arc<dyn db::Repository + Send + Sync>,
    ) = {
        let sqlite = db::SqliteRepo::new(db_path);
        if cli.reset {
            let _ = sqlite.reset_all();
        }
        let _ = sqlite.import_if_needed();
        (
            Arc::new(sqlite.clone()) as Arc<dyn Storage + Send + Sync>,
            Arc::new(sqlite) as Arc<dyn db::Repository + Send + Sync>,
        )
    };

    if let Ok(Some(last)) = store.load_last() {
        println!("📦 Resuming from height {}", last.height + 1);
    }

    let mut scanner = scanner::Scanner::new(&client, confirmations, debug);
    if let Ok(Some(last)) = store.load_last() {
        scanner.start_from(last.height + 1);
    }

    let store2 = store.clone();
    let repo2 = repo.clone();
    let batch_size = cli.batch_size;
    if batch_size <= 1 {
        scanner.run(|height, block, hash| {
            if let Ok(Some(prev)) = store2.load_last() {
                if is_orphan(&prev, block) {
                    eprintln!(
                        "error: detected orphan branch at height {}: parent {} != last processed {}",
                        height, block.header.prev_blockhash, prev.hash
                    );
                    std::process::exit(1);
                }
            }
            process_block(repo2.clone(), block, debug, height, hash);
            if let Err(e) = store2.save_last(height, hash) {
                eprintln!(
                    "warning: failed to save last block {} ({}): {}",
                    height, hash, e
                );
            }
        });
    } else {
        scanner.run_batch(batch_size, |items| {
            if let Ok(Some(prev)) = store2.load_last() {
                let mut expected = prev.hash.clone();
                for (h, b, hs) in items.iter() {
                    if b.header.prev_blockhash.to_string() != expected {
                        eprintln!(
                            "error: detected orphan branch at height {}: parent {} != last processed {}",
                            h, b.header.prev_blockhash, expected
                        );
                        std::process::exit(1);
                    }
                    expected = hs.clone();
                }
            }
            let refs: Vec<(u64, &Block, &str)> = items
                .iter()
                .map(|(h, b, hs)| (*h, b, hs.as_str()))
                .collect();
            parser::parse_blocks_batch(repo2.as_ref(), &refs);
            if let Some((last_h, _b, last_hash)) = items.last() {
                if let Err(e) = store2.save_last(*last_h, last_hash) {
                    eprintln!(
                        "warning: failed to save last block {} ({}): {}",
                        last_h, last_hash, e
                    );
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use bitcoin::{OutPoint, Transaction, TxIn, TxOut};

    #[test]
    fn format_block_scripts_includes_scripts() {
        let script_sig = bitcoin::ScriptBuf::from_bytes(vec![0x01, 0x02, 0x03]);
        let script_pubkey = bitcoin::ScriptBuf::from_bytes(vec![0x51]);

        let txin = TxIn {
            previous_output: OutPoint::null(),
            script_sig: script_sig.clone(),
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::default(),
        };
        let txout = TxOut {
            value: bitcoin::Amount::from_sat(0),
            script_pubkey: script_pubkey.clone(),
        };
        let tx = Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![txin],
            output: vec![txout],
        };
        let header = bitcoin::block::Header {
            version: bitcoin::block::Version::TWO,
            prev_blockhash: bitcoin::BlockHash::all_zeros(),
            merkle_root: bitcoin::TxMerkleNode::all_zeros(),
            time: 0,
            bits: bitcoin::CompactTarget::from_consensus(0),
            nonce: 0,
        };
        let block = Block {
            header,
            txdata: vec![tx],
        };

        let out = format_block_scripts(&block);
        assert!(out.contains(&hex::encode(script_sig.as_bytes())));
        assert!(out.contains(&hex::encode(script_pubkey.as_bytes())));
    }

    fn make_block_with_prev(prev: bitcoin::BlockHash) -> Block {
        let header = bitcoin::block::Header {
            version: bitcoin::block::Version::TWO,
            prev_blockhash: prev,
            merkle_root: bitcoin::TxMerkleNode::all_zeros(),
            time: 0,
            bits: bitcoin::CompactTarget::from_consensus(0),
            nonce: 0,
        };
        Block {
            header,
            txdata: vec![],
        }
    }

    #[test]
    fn orphan_detection_matches_parent() {
        let b0 = make_block_with_prev(bitcoin::BlockHash::all_zeros());
        let b0_hash = b0.header.block_hash();
        let b1 = make_block_with_prev(b0_hash);
        let last = crate::storage::LastBlock {
            height: 0,
            hash: b0_hash.to_string(),
        };
        assert!(!is_orphan(&last, &b1));
    }

    #[test]
    fn orphan_detection_flags_mismatch() {
        let b0 = make_block_with_prev(bitcoin::BlockHash::all_zeros());
        let b0_hash = b0.header.block_hash();
        let b1 = make_block_with_prev(b0_hash);
        let last = crate::storage::LastBlock {
            height: 0,
            hash: "deadbeef".to_string(),
        };
        assert!(is_orphan(&last, &b1));
    }
}
