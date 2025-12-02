use bitcoincore_rpc::{Auth, Client, RpcApi};
use tempfile::TempDir;
use testcontainers::runners::SyncRunner;

mod common;

#[test]
fn e2e_send_amount() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);
    let auth = Auth::UserPass("dev".into(), "dev".into());

    let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

    // Wallet A: create and fund
    let data_dir_a = TempDir::new().expect("temp dir");
    let output = common::base_cmd(&rpc_url, &data_dir_a)
        .arg("wallet")
        .arg("init")
        .arg("--passphrase")
        .arg("passphrase")
        .arg("--mnemonic")
        .arg("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about")
        .output()
        .expect("run wallet init");
    assert!(output.status.success());

    let addr_a = common::wallet_address(&rpc_url, &data_dir_a);

    // Mine coins to wallet A so it has UTXOs
    root_client.generate_to_address(101, &addr_a).expect("mine");

    // Wallet B: create and get receive address
    let data_dir_b = TempDir::new().expect("temp dir");
    let output = common::base_cmd(&rpc_url, &data_dir_b)
        .arg("wallet")
        .arg("init")
        .arg("--passphrase")
        .arg("passphrase")
        .arg("--mnemonic")
        .arg("spread scrub pepper awful hint scan oil mystery push dignity again tomato")
        .output()
        .expect("run wallet init B");
    assert!(output.status.success());

    let addr_b = common::wallet_address(&rpc_url, &data_dir_b);

    // Send some amount from A to B using the CLI
    let output = common::base_cmd(&rpc_url, &data_dir_a)
        .arg("tx")
        .arg("send-amount")
        .arg(addr_b.to_string())
        .arg("--amount-sat")
        .arg("10000")
        .arg("--passphrase")
        .arg("passphrase")
        .output()
        .expect("run tx send-amount");
    assert!(output.status.success(), "{:?}", output);

    // Mine a block to confirm
    root_client
        .generate_to_address(1, &addr_a)
        .expect("mine confirm");

    // Check wallet B balance shows the received funds (trusted)
    let output = common::base_cmd(&rpc_url, &data_dir_b)
        .arg("wallet")
        .arg("balance")
        .output()
        .expect("run wallet balance B");
    assert!(output.status.success(), "{:?}", output);
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", out, err);
    assert!(combined.contains("trusted: 10000 SAT"), "{}", combined);
}
