use std::env;

use bitcoincore_rpc::{Auth, Client};
use dotenvy::dotenv;
use bitcoin::Block;
mod cli;
mod scanner;
mod parser;

fn format_block_scripts(block: &Block) -> String {
    let mut out = String::new();
    for tx in &block.txdata {
        let txid = tx.compute_txid();
        out.push_str(&format!("tx {}:\n", txid));
        for (i, input) in tx.input.iter().enumerate() {
            let script_sig_hex = hex::encode(input.script_sig.as_bytes());
            out.push_str(&format!("  vin[{}] scriptSig: {}\n", i, script_sig_hex));
            if !input.witness.is_empty() {
                let wit_items: Vec<String> = input
                    .witness
                    .iter()
                    .map(|w| hex::encode(w.as_ref()))
                    .collect();
                out.push_str(&format!("  vin[{}] witness: [{}]\n", i, wit_items.join(", ")));
            }
        }
        for (j, output) in tx.output.iter().enumerate() {
            let script_pubkey_hex = hex::encode(output.script_pubkey.as_bytes());
            out.push_str(&format!("  vout[{}] scriptPubKey: {}\n", j, script_pubkey_hex));
        }
    }
    out
}

fn process_block(block: &Block, debug: bool, height: u64, block_hash_str: &str) {
    if debug {
        let s = format_block_scripts(block);
        print!("{}", s);
    } else {
        parser::parse(height, block, block_hash_str);
    }
}

fn main() {
    dotenv().ok();

    let cli = cli::parse();
    let debug = cli.debug;
    let confirmations = cli.confirmations;

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
    println!("🛠️ Debug: {}", if debug { "on" } else { "off" });
    println!("🧮 Confirmations: {}", confirmations);

    let client = Client::new(&rpc_url, auth).expect("failed to create RPC client");

    let mut scanner = scanner::Scanner::new(&client, confirmations, debug);
    scanner.run(|height, block, hash| {
        process_block(block, debug, height, hash);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{Transaction, TxIn, TxOut, OutPoint};
    use bitcoin::hashes::Hash;

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
}
