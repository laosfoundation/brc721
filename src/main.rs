use std::env;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bitcoin;
use bitcoincore_rpc::{Auth, Client};
use dotenvy::dotenv;
mod api;
mod cli;
mod parser;
mod scanner;
mod storage;
mod wallet;
use crate::scanner::BitcoinRpc;
use crate::storage::Storage;

fn is_orphan(prev: &storage::Block, block: &bitcoin::Block) -> bool {
    block.header.prev_blockhash.to_string() != prev.hash
}

fn format_block_scripts(block: &bitcoin::Block) -> String {
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
    storage: Arc<dyn Storage + Send + Sync>,
    block: &bitcoin::Block,
    debug: bool,
    height: u64,
    block_hash: &bitcoin::BlockHash,
) {
    if debug {
        let s = format_block_scripts(block);
        print!("{}", s);
    } else {
        parser::parse_with_repo(storage.as_ref(), height, block, block_hash)
    }
}

#[tokio::main]
async fn main() {
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

    let client = Client::new(&rpc_url, auth.clone()).expect("failed to create RPC client");

    if let Some(cmd) = &cli.command {
        match cmd {
            crate::cli::Command::WalletInit { name } => {
                let _ = wallet::Wallet::create_wallet(&client, name);
                println!("wallet {} ready", name);
                return;
            }
            crate::cli::Command::WalletNewAddress { name } => {
                let url = format!("{}/wallet/{}", rpc_url.trim_end_matches('/'), name);
                let wclient = Client::new(&url, auth.clone()).expect("wallet-scoped client");
                let addr = wallet::Wallet::new_address(&wclient).expect("new address");
                println!("{}", addr);
                return;
            }
            crate::cli::Command::WalletBalance { name } => {
                let url = format!("{}/wallet/{}", rpc_url.trim_end_matches('/'), name);
                let wclient = Client::new(&url, auth.clone()).expect("wallet-scoped client");
                let b = wallet::Wallet::balance(&wclient).expect("balance");
                println!("{}", b);
                return;
            }
            crate::cli::Command::CollectionCreate {
                laos_hex,
                rebaseable,
                fee_rate,
                name,
            } => {
                let mut laos = [0u8; 20];
                let bytes = hex::decode(laos_hex).expect("hex");
                assert_eq!(bytes.len(), 20, "laos hex must be 20 bytes");
                laos.copy_from_slice(&bytes);
                let url = format!("{}/wallet/{}", rpc_url.trim_end_matches('/'), name);
                let wclient = Client::new(&url, auth.clone()).expect("wallet-scoped client");
                let txid = wallet::Wallet::create_and_broadcast_collection(
                    &wclient,
                    laos,
                    *rebaseable,
                    *fee_rate,
                )
                .expect("broadcast");
                println!("{}", txid);
                return;
            }
            crate::cli::Command::Serve { bind } => {
                let bind_addr = bind
                    .clone()
                    .or_else(|| env::var("BRC721_API_BIND").ok())
                    .unwrap_or_else(|| "127.0.0.1:8080".to_string());
                let token = env::var("BRC721_API_TOKEN").ok();

                let default_db = "./.brc721/brc721.sqlite".to_string();
                let db_path = env::var("BRC721_DB_PATH").unwrap_or(default_db);
                let sqlite = storage::SqliteStorage::new(db_path);
                if cli.reset {
                    let _ = sqlite.reset_all();
                }
                let _ = sqlite.init();
                let storage_arc: Arc<dyn Storage + Send + Sync> = Arc::new(sqlite);

                if let Ok(Some(last)) = storage_arc.load_last() {
                    println!("📦 Resuming from height {}", last.height + 1);
                }

                let rpc_url2 = rpc_url.clone();
                let auth2 = auth.clone();
                let storage2 = storage_arc.clone();
                let debug2 = debug;
                let confirmations2 = confirmations;
                let batch_size2 = cli.batch_size;

                std::thread::spawn(move || {
                    let client2 =
                        Client::new(&rpc_url2, auth2).expect("failed to create RPC client");
                    let mut scanner = scanner::Scanner::new(&client2, confirmations2, debug2);
                    if let Ok(Some(last)) = storage2.load_last() {
                        scanner.start_from(last.height + 1);
                    }
                    let st = storage2.clone();
                    if batch_size2 <= 1 {
                        loop {
                            let blocks = match scanner.next_blocks(1) {
                                Ok(blocks) => blocks,
                                Err(e) => {
                                    eprintln!("scanner next_blocks error: {}", e);
                                    thread::sleep(Duration::from_millis(500));
                                    continue;
                                }
                            };
                            if blocks.is_empty() {
                                match client2.wait_for_new_block(60) {
                                    Ok(()) => {}
                                    Err(_e) => thread::sleep(Duration::from_secs(1)),
                                }
                                continue;
                            }
                            for (height, block, hash) in blocks {
                                if let Ok(Some(prev)) = st.load_last() {
                                    if is_orphan(&prev, &block) {
                                        eprintln!(
                                            "error: detected orphan branch at height {}: parent {} != last processed {}",
                                            height, block.header.prev_blockhash, prev.hash
                                        );
                                        std::process::exit(1);
                                    }
                                }
                                process_block(st.clone(), &block, debug2, height, &hash);
                                let hash_str = hash.to_string();
                                if let Err(e) = st.save_last(height, &hash_str) {
                                    eprintln!(
                                        "warning: failed to save last block {} ({}): {}",
                                        height, hash_str, e
                                    );
                                }
                            }
                        }
                    } else {
                        loop {
                            let items = match scanner.next_blocks(batch_size2) {
                                Ok(items) => items,
                                Err(e) => {
                                    eprintln!("scanner next_blocks error: {}", e);
                                    thread::sleep(Duration::from_millis(500));
                                    continue;
                                }
                            };
                            if items.is_empty() {
                                match client2.wait_for_new_block(60) {
                                    Ok(()) => {}
                                    Err(_e) => thread::sleep(Duration::from_secs(1)),
                                }
                                continue;
                            }
                            if let Ok(Some(prev)) = st.load_last() {
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
                            let refs: Vec<(u64, &bitcoin::Block, &bitcoin::BlockHash)> = items
                                .iter()
                                .map(|(h, b, hs)| (*h, b, hs))
                                .collect();
                            parser::parse_blocks_batch(st.as_ref(), &refs);
                            if let Some((last_h, _b, last_hash)) = items.last() {
                                let last_hash_str = last_hash.to_string();
                                if let Err(e) = st.save_last(*last_h, &last_hash_str) {
                                    eprintln!(
                                        "warning: failed to save last block {} ({}): {}",
                                        last_h, last_hash_str, e
                                    );
                                }
                            }
                        }
                    }
                });

                if let Err(e) = api::serve(bind_addr, rpc_url.clone(), auth.clone(), token).await {
                    eprintln!("api server error: {}", e);
                    std::process::exit(1);
                }
                return;
            }
        }
    }

    let default_db = "./.brc721/brc721.sqlite".to_string();
    let db_path = env::var("BRC721_DB_PATH").unwrap_or(default_db);

    let storage_arc: Arc<dyn Storage + Send + Sync> = {
        let sqlite = storage::SqliteStorage::new(db_path);
        if cli.reset {
            let _ = sqlite.reset_all();
        }
        let _ = sqlite.init();
        Arc::new(sqlite)
    };

    if let Ok(Some(last)) = storage_arc.load_last() {
        println!("📦 Resuming from height {}", last.height + 1);
    }

    let mut scanner = scanner::Scanner::new(&client, confirmations, debug);
    if let Ok(Some(last)) = storage_arc.load_last() {
        scanner.start_from(last.height + 1);
    }

    let storage2 = storage_arc.clone();
    let batch_size = cli.batch_size;
    if batch_size <= 1 {
        loop {
            let blocks = match scanner.next_blocks(1) {
                Ok(blocks) => blocks,
                Err(e) => {
                    eprintln!("scanner next_blocks error: {}", e);
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };
            if blocks.is_empty() {
                match client.wait_for_new_block(60) {
                    Ok(()) => {}
                    Err(_e) => thread::sleep(Duration::from_secs(1)),
                }
                continue;
            }
            for (height, block, hash) in blocks {
                if let Ok(Some(prev)) = storage2.load_last() {
                    if is_orphan(&prev, &block) {
                        eprintln!(
                            "error: detected orphan branch at height {}: parent {} != last processed {}",
                            height, block.header.prev_blockhash, prev.hash
                        );
                        std::process::exit(1);
                    }
                }
                process_block(storage2.clone(), &block, debug, height, &hash);
                let hash_str = hash.to_string();
                if let Err(e) = storage2.save_last(height, &hash_str) {
                    eprintln!(
                        "warning: failed to save last block {} ({}): {}",
                        height, hash_str, e
                    );
                }
            }
        }
    } else {
        loop {
            let items = match scanner.next_blocks(batch_size) {
                Ok(items) => items,
                Err(e) => {
                    eprintln!("scanner next_blocks error: {}", e);
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };
            if items.is_empty() {
                match client.wait_for_new_block(60) {
                    Ok(()) => {}
                    Err(_e) => thread::sleep(Duration::from_secs(1)),
                }
                continue;
            }
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
            let refs: Vec<(u64, &bitcoin::Block, &bitcoin::BlockHash)> = items
                .iter()
                .map(|(h, b, hs)| (*h, b, hs))
                .collect();
            parser::parse_blocks_batch(storage2.as_ref(), &refs);
            if let Some((last_h, _b, last_hash)) = items.last() {
                let last_hash_str = last_hash.to_string();
                if let Err(e) = storage2.save_last(*last_h, &last_hash_str) {
                    eprintln!(
                        "warning: failed to save last block {} ({}): {}",
                        last_h, last_hash_str, e
                    );
                }
            }
        }
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
        let block = bitcoin::Block {
            header,
            txdata: vec![tx],
        };

        let out = format_block_scripts(&block);
        assert!(out.contains(&hex::encode(script_sig.as_bytes())));
        assert!(out.contains(&hex::encode(script_pubkey.as_bytes())));
    }

    fn make_block_with_prev(prev: bitcoin::BlockHash) -> bitcoin::Block {
        let header = bitcoin::block::Header {
            version: bitcoin::block::Version::TWO,
            prev_blockhash: prev,
            merkle_root: bitcoin::TxMerkleNode::all_zeros(),
            time: 0,
            bits: bitcoin::CompactTarget::from_consensus(0),
            nonce: 0,
        };
        bitcoin::Block {
            header,
            txdata: vec![],
        }
    }

    #[test]
    fn orphan_detection_matches_parent() {
        let b0 = make_block_with_prev(bitcoin::BlockHash::all_zeros());
        let b0_hash = b0.header.block_hash();
        let b1 = make_block_with_prev(b0_hash);
        let last = crate::storage::Block {
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
        let last = crate::storage::Block {
            height: 0,
            hash: "deadbeef".to_string(),
        };
        assert!(is_orphan(&last, &b1));
    }
}
