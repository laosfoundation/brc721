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
fn e2e_balance() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);
    let auth = common::auth();

    let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

    let data_dir = TempDir::new().expect("temp dir");
    let stdout = common::base_cmd(&rpc_url, &data_dir)
        .arg("wallet")
        .arg("init")
        .arg("--mnemonic")
        .arg(MNEMONIC)
        .output()
        .expect("run wallet init");
    assert!(stdout.status.success());

    let addr = Address::from_str(PAYING_ADDRESS)
        .expect("address")
        .assume_checked();
    root_client.generate_to_address(101, &addr).expect("mine");

    // 2) Query the app balance command; it should reflect the same totals as Core.
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("wallet")
        .arg("balance")
        .output()
        .expect("run wallet balance");
    assert!(output.status.success());

    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(out.contains("Loaded env from .env"));

    let combined = format!("{}{}", out, err);
    assert!(
        combined.contains("trusted: 5000000000") && combined.contains("immature: 500000000000"),
        "balance output not found in stdout/stderr:\nstdout:\n{}\nstderr:\n{}",
        out,
        err
    );
}
