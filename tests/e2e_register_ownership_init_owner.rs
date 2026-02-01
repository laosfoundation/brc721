use bitcoin::Address;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde_json::json;
use std::fs;
use std::process::Output;
use std::str::FromStr;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use testcontainers::runners::SyncRunner;

mod common;

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn combined_output(output: &Output) -> String {
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    format!("{}{}", out, err)
}

fn parse_txid(output: &Output) -> bitcoin::Txid {
    let combined = combined_output(output);
    for line in combined.lines() {
        if line.contains("txid:") {
            let txid_str = line
                .split_whitespace()
                .last()
                .expect("txid token at end of line");
            return txid_str.parse().expect("txid");
        }
    }
    panic!("txid not found in output:\n{}", combined);
}

fn parse_owner_output_address(output: &Output) -> Address {
    let combined = combined_output(output);
    for line in combined.lines() {
        if let Some(pos) = line.find("owner_output=") {
            let rest = &line[pos + "owner_output=".len()..];
            let end = rest.find(',').unwrap_or(rest.len());
            let addr_str = rest[..end].trim();
            return Address::from_str(addr_str)
                .expect("owner address")
                .assume_checked();
        }
    }
    panic!("owner_output not found in output:\n{}", combined);
}

fn collection_id_for_confirmed_tx(root: &Client, txid: &bitcoin::Txid) -> (u64, u32) {
    let txid_str = txid.to_string();
    let tx_verbose: serde_json::Value = root
        .call("getrawtransaction", &[json!(txid_str), json!(true)])
        .expect("getrawtransaction verbose");
    let blockhash = tx_verbose
        .get("blockhash")
        .and_then(|v| v.as_str())
        .expect("blockhash present after confirmation");

    let block: serde_json::Value = root
        .call("getblock", &[json!(blockhash), json!(1)])
        .expect("getblock");

    let height = block
        .get("height")
        .and_then(|v| v.as_u64())
        .expect("block height");

    let txs = block
        .get("tx")
        .and_then(|v| v.as_array())
        .expect("tx array");
    let tx_index = txs
        .iter()
        .position(|v| v.as_str() == Some(txid_str.as_str()))
        .expect("txid must be in its block") as u32;

    (height, tx_index)
}

#[test]
fn e2e_register_ownership_init_owner_rejects_nft_utxo() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);
    let auth = Auth::UserPass("dev".into(), "dev".into());

    let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

    // Wallet: create and fund to pay fees
    let data_dir = TempDir::new().expect("temp dir");
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("wallet")
        .arg("init")
        .arg("--passphrase")
        .arg("passphrase")
        .arg("--mnemonic")
        .arg(MNEMONIC)
        .output()
        .expect("run wallet init");
    assert!(output.status.success(), "{:?}", output);

    let addr = common::wallet_address(&rpc_url, &data_dir);
    root_client.generate_to_address(101, &addr).expect("mine");

    let log_path = data_dir.path().join("daemon.log");
    let mut daemon = common::start_daemon(&rpc_url, &data_dir, Some(&log_path));
    common::wait_for_scanner_db(&data_dir);

    // Register a collection so we can use a real collection id (HEIGHT:TX_INDEX)
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("tx")
        .arg("register-collection")
        .arg("--evm-collection-address")
        .arg("0xffff0123ffffffffffffffffffffffff3210ffff")
        .arg("--passphrase")
        .arg("passphrase")
        .output()
        .expect("run tx register-collection");
    assert!(output.status.success(), "{:?}", output);
    let collection_txid = parse_txid(&output);

    // Mine a block to confirm the collection tx, then compute (height, tx_index)
    root_client
        .generate_to_address(1, &addr)
        .expect("mine confirm collection");
    let (collection_height, collection_tx_index) =
        collection_id_for_confirmed_tx(&root_client, &collection_txid);
    let collection_id = format!("{collection_height}:{collection_tx_index}");

    // Register ownership to create an ownership UTXO (vout1).
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("tx")
        .arg("register-ownership")
        .arg("--collection-id")
        .arg(&collection_id)
        .arg("--slots")
        .arg("0")
        .arg("--passphrase")
        .arg("passphrase")
        .output()
        .expect("run tx register-ownership");
    assert!(output.status.success(), "{:?}", output);
    let ownership_txid = parse_txid(&output);
    let owner_address = parse_owner_output_address(&output);

    // Confirm the ownership tx so the scanner can index it.
    root_client
        .generate_to_address(1, &addr)
        .expect("mine confirm ownership");
    let (ownership_height, ownership_tx_index) =
        collection_id_for_confirmed_tx(&root_client, &ownership_txid);

    let deadline = Instant::now() + Duration::from_secs(20);
    let needle = format!(
        "register-ownership indexed (block {} tx {}",
        ownership_height, ownership_tx_index
    );
    loop {
        if let Ok(contents) = fs::read_to_string(&log_path) {
            if contents.contains(&needle) {
                break;
            }
        }
        if let Some(status) = daemon.try_wait() {
            panic!("daemon exited early: {}", status);
        }
        if Instant::now() > deadline {
            panic!(
                "timed out waiting for scanner to index ownership tx at {}#{} (log={})",
                ownership_height,
                ownership_tx_index,
                log_path.display()
            );
        }
        sleep(Duration::from_millis(100));
    }

    daemon.stop();

    // Attempt to reuse the ownership address as init-owner. This must fail because
    // it only has an ownership UTXO, which is disallowed for input0.
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("tx")
        .arg("register-ownership")
        .arg("--collection-id")
        .arg(&collection_id)
        .arg("--slots")
        .arg("1")
        .arg("--init-owner")
        .arg(owner_address.to_string())
        .arg("--passphrase")
        .arg("passphrase")
        .output()
        .expect("run tx register-ownership with init-owner");
    assert!(
        !output.status.success(),
        "expected failure when init-owner only has an ownership UTXO"
    );
    let combined = combined_output(&output);
    assert!(
        combined.contains("no spendable UTXO found for init-owner address"),
        "unexpected output:\n{}",
        combined
    );
}
