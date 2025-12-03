use std::str::FromStr;

use bitcoin::{BlockHash, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use tempfile::TempDir;
use testcontainers::runners::SyncRunner;

mod common;

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

#[test]
fn e2e_register_ownership() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);
    let auth = Auth::UserPass("dev".into(), "dev".into());
    let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

    // Wallet init + funding
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
    assert!(output.status.success());

    let funding_addr = common::wallet_address(&rpc_url, &data_dir);
    root_client
        .generate_to_address(101, &funding_addr)
        .expect("mine funding blocks");

    // Register collection
    let register_output = common::base_cmd(&rpc_url, &data_dir)
        .arg("tx")
        .arg("register-collection")
        .arg("--evm-collection-address")
        .arg("0xffff0123ffffffffffffffffffffffff3210ffff")
        .arg("--passphrase")
        .arg("passphrase")
        .output()
        .expect("run tx register-collection");
    assert!(register_output.status.success(), "{:?}", register_output);
    let collection_txid = extract_txid(&register_output);

    let collection_block_hash = mine_one_block(&root_client, &funding_addr);
    let collection_height = root_client.get_block_count().expect("block count");
    let collection_tx_index =
        locate_tx_index(&root_client, &collection_block_hash, &collection_txid);
    let collection_id = format!("{}:{}", collection_height, collection_tx_index);

    // Prepare ownership assignments
    let owner_addr_1 = common::wallet_address(&rpc_url, &data_dir);
    let owner_addr_2 = common::wallet_address(&rpc_url, &data_dir);

    let ownership_output = common::base_cmd(&rpc_url, &data_dir)
        .arg("tx")
        .arg("register-ownership")
        .arg("--collection-id")
        .arg(&collection_id)
        .arg("--assignment")
        .arg(format!("{}@700:0-3", owner_addr_1))
        .arg("--assignment")
        .arg(format!("{}:4-5,10", owner_addr_2))
        .arg("--passphrase")
        .arg("passphrase")
        .output()
        .expect("run tx register-ownership");
    assert!(ownership_output.status.success(), "{:?}", ownership_output);
    let ownership_txid = extract_txid(&ownership_output);

    let ownership_block_hash = mine_one_block(&root_client, &funding_addr);
    let _ownership_height = root_client.get_block_count().expect("block count");
    locate_tx_index(&root_client, &ownership_block_hash, &ownership_txid);
}

fn extract_txid(output: &std::process::Output) -> String {
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let needle = "txid:";
    let idx = combined
        .find(needle)
        .expect("txid substring present in CLI output");
    let start = idx + needle.len();
    combined[start..]
        .split_whitespace()
        .next()
        .expect("txid token")
        .trim()
        .trim_matches(|c: char| c == ',' || c == '.')
        .to_string()
}

fn mine_one_block(client: &Client, addr: &bitcoin::Address) -> BlockHash {
    let hashes = client
        .generate_to_address(1, addr)
        .expect("mine single block");
    hashes[0]
}

fn locate_tx_index(client: &Client, block_hash: &BlockHash, txid_str: &str) -> usize {
    let txid = Txid::from_str(txid_str).expect("txid parse");
    let block = client.get_block(block_hash).expect("fetch block");
    block
        .txdata
        .iter()
        .position(|tx| tx.compute_txid() == txid)
        .expect("tx present in block")
}
