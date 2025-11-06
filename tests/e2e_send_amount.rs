use std::str::FromStr;

use bitcoin::Address;
use bitcoincore_rpc::{Client, RpcApi};
use tempfile::TempDir;
use testcontainers::runners::SyncRunner;

mod common;

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PAYING_ADDRESS: &str = "bcrt1p8wpt9v4frpf3tkn0srd97pksgsxc5hs52lafxwru9kgeephvs7rqjeprhg";

#[test]
fn e2e_send_amount() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);
    let auth = common::auth();

    let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

    let data_dir = TempDir::new().expect("temp dir");
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("wallet")
        .arg("init")
        .arg("--mnemonic")
        .arg(MNEMONIC)
        .output()
        .expect("run wallet init");
    assert!(output.status.success());

    let mined_addr = common::wallet_address(&rpc_url, &data_dir);

    // Mine coins to that address so wallet has UTXOs
    root_client
        .generate_to_address(101, &mined_addr)
        .expect("mine");

    // Now send some amount to another address using the CLI
    let target = Address::from_str(PAYING_ADDRESS)
        .expect("address")
        .assume_checked();

    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("tx")
        .arg("send-amount")
        .arg(target.to_string())
        .arg("--amount-sat")
        .arg("10000")
        .output()
        .expect("run tx send-amount");
    assert!(output.status.success(), "{:?}", output);
}
