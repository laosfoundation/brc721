use std::env;
use std::thread;
use std::time::Duration;

use bitcoincore_rpc::{Auth, Client, RpcApi};
use dotenvy::dotenv;
use bitcoin::Block;

fn main() {
    dotenv().ok();

    let rpc_url = env::var("BITCOIN_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8332".to_string());

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

    let client = Client::new(&rpc_url, auth).expect("failed to create RPC client");

    let mut current_height: u64 = 0;

    loop {
        match client.get_block_count() {
            Ok(tip) => {
                while current_height <= tip {
                    match client.get_block_hash(current_height) {
                        Ok(hash) => {
                            match client.get_block(&hash) {
                                Ok(block) => {
                                    let block: Block = block;
                                    println!("{} {}", hash, block.txdata.len());
                                    for tx in &block.txdata {
                                        let txid = tx.compute_txid();
                                        println!("tx {}:", txid);
                                        for (i, input) in tx.input.iter().enumerate() {
                                            let script_sig_hex = hex::encode(input.script_sig.as_bytes());
                                            println!("  vin[{}] scriptSig: {}", i, script_sig_hex);
                                            if let Some(wit) = (!input.witness.is_empty()).then(|| &input.witness) {
                                                let mut wit_items: Vec<String> = Vec::new();
                                                for w in wit.iter() {
                                                    wit_items.push(hex::encode(w.as_ref()));
                                                }
                                                println!("  vin[{}] witness: [{}]", i, wit_items.join(", "));
                                            }
                                        }
                                        for (j, output) in tx.output.iter().enumerate() {
                                            let script_pubkey_hex = hex::encode(output.script_pubkey.as_bytes());
                                            println!("  vout[{}] scriptPubKey: {}", j, script_pubkey_hex);
                                        }
                                    }
                                    current_height += 1;
                                }
                                Err(e) => {
                                    eprintln!("error get_block at height {}: {}", current_height, e);
                                    thread::sleep(Duration::from_millis(500));
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("error get_block_hash at height {}: {}", current_height, e);
                            thread::sleep(Duration::from_millis(500));
                        }
                    }
                }

                match client.wait_for_new_block(60) {
                    Ok(_br) => {}
                    Err(_e) => {
                        thread::sleep(Duration::from_secs(1));
                    }
                }
            }
            Err(e) => {
                eprintln!("error get_block_count: {}", e);
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
