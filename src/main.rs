use std::sync::Arc;

use bitcoincore_rpc::{Auth, Client};
mod cli;
mod core;
mod parser;
mod scanner;
mod storage;
use storage::Storage;

#[cfg(test)]
fn is_orphan(prev: &storage::Block, block: &bitcoin::Block) -> bool {
    block.header.prev_blockhash.to_string() != prev.hash
}

#[cfg(test)]
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

fn main() {
    let cli = cli::parse();

    let auth_mode = match (&cli.rpc_user, &cli.rpc_pass) {
        (Some(_), Some(_)) => "user/pass",
        _ => "none",
    };

    println!("🚀 Starting brc721");
    println!("🔗 RPC URL: {}", cli.rpc_url);
    println!("🔐 Auth: {}", auth_mode);
    println!("🛠️ Debug: {}", if cli.debug { "on" } else { "off" });
    println!("🧮 Confirmations: {}", cli.confirmations);
    println!("📂 Data dir: {}", cli.data_dir);

    init_data_dir(&cli);
    let storage_arc = init_storage(&cli);
    let starting_block = storage_arc
        .load_last()
        .unwrap_or_default()
        .map(|last| last.height + 1)
        .unwrap_or_default();
    let scanner = init_scanner(&cli, starting_block);

    let core = core::Core::new(storage_arc.clone(), scanner, cli.debug, cli.batch_size);
    core.run();
}

fn init_data_dir(cli: &cli::Cli) {
    let data_dir = std::path::PathBuf::from(&cli.data_dir);
    let _ = std::fs::create_dir_all(&data_dir);
}

fn init_storage(cli: &cli::Cli) -> Arc<dyn Storage + Send + Sync> {
    let data_dir = std::path::PathBuf::from(&cli.data_dir);
    let db_path = data_dir
        .join("brc721.sqlite")
        .to_string_lossy()
        .into_owned();
    let sqlite = storage::SqliteStorage::new(&db_path);
    if cli.reset {
        let _ = sqlite.reset_all();
    }
    let _ = sqlite.init();
    Arc::new(sqlite)
}

fn init_scanner(cli: &cli::Cli, start_block: u64) -> scanner::Scanner<Client> {
    let auth = match (&cli.rpc_user, &cli.rpc_pass) {
        (Some(user), Some(pass)) => Auth::UserPass(user.clone(), pass.clone()),
        _ => Auth::None,
    };

    let client = Client::new(&cli.rpc_url, auth).expect("failed to create RPC client");
    scanner::Scanner::new(client)
        .with_confirmations(cli.confirmations)
        .with_capacity(cli.batch_size)
        .with_start_from(start_block)
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
