use std::env;
use std::thread;
use std::time::Duration;

use bitcoincore_rpc::{Auth, Client, RpcApi};
use dotenvy::dotenv;

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

    let client = Client::new(&rpc_url, auth).expect("failed to create RPC client");

    let mut current_height: u64 = 0;

    loop {
        match client.get_block_count() {
            Ok(tip) => {
                while current_height <= tip {
                    match client.get_block_hash(current_height) {
                        Ok(hash) => {
                            match client.get_block_header_info(&hash) {
                                Ok(info) => {
                                    println!("{} {}", hash, info.n_tx);
                                    current_height += 1;
                                }
                                Err(e) => {
                                    eprintln!("error get_block_header_info at height {}: {}", current_height, e);
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
