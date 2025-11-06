use std::process::Command as ProcCommand;
use std::str::FromStr;

use bitcoin::Address;
use bitcoincore_rpc::{Client, RpcApi};
use tempfile::TempDir;
use testcontainers::runners::SyncRunner;

mod common;

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PAYING_ADDRESS: &str = "bcrt1p8wpt9v4frpf3tkn0srd97pksgsxc5hs52lafxwru9kgeephvs7rqjeprhg";

fn base_cmd(rpc_url: &String, data_dir: &TempDir) -> ProcCommand {
    let mut command = ProcCommand::new("cargo");

    command
        .arg("run")
        .arg("--quiet")
        .arg("--")
        .arg("--network")
        .arg("regtest")
        .arg("--data-dir")
        .arg(data_dir.path())
        .arg("--rpc-url")
        .arg(&rpc_url)
        .arg("--rpc-user")
        .arg("dev")
        .arg("--rpc-pass")
        .arg("dev");

    command
}

#[test]
fn e2e_balance() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);
    let auth = common::auth();

    let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

    let data_dir = TempDir::new().expect("temp dir");
    let stdout = base_cmd(&rpc_url, &data_dir)
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
    let stdout = base_cmd(&rpc_url, &data_dir)
        .arg("wallet")
        .arg("balance")
        .output()
        .expect("run wallet balance");
    assert!(stdout.status.success());

    let stdout = String::from_utf8_lossy(&stdout.stdout);
    // Expect the balances debug to include trusted and immature fields
    assert_eq!(stdout, "Loaded env from .env\nGetBalancesResult { mine: GetBalancesResultEntry { trusted: 5000000000 SAT, untrusted_pending: 0 SAT, immature: 500000000000 SAT }, watchonly: None }\n");
}
