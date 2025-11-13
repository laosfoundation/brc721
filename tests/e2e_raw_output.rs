use bitcoincore_rpc::{Client, RpcApi};
use tempfile::TempDir;
use testcontainers::runners::SyncRunner;

mod common;

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

#[test]
fn e2e_raw_output() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);
    let auth = common::auth();

    let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

    // Wallet: create and fund
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

    let addr = common::wallet_address(&rpc_url, &data_dir);

    // Fund wallet so it can broadcast
    root_client.generate_to_address(101, &addr).expect("mine");

    // Send raw OP_RETURN output with custom payload
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("tx")
        .arg("raw-output")
        .arg("--hex")
        .arg("deadbeef")
        .arg("--passphrase")
        .arg("passphrase")
        .output()
        .expect("run tx raw-output");
    assert!(output.status.success(), "{:?}", output);

    // Mine a block to include it
    root_client
        .generate_to_address(1, &addr)
        .expect("mine confirm");
}
