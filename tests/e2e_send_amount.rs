use bitcoincore_rpc::Client;
use tempfile::TempDir;
use testcontainers::runners::SyncRunner;

mod common;

#[test]
fn e2e_send_amount() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);
    let auth = common::auth();

    let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

    let data_dir = TempDir::new().expect("temp dir");
    let stdout = common::base_cmd(&rpc_url, &data_dir)
        .arg("wallet")
        .arg("init")
        .output()
        .expect("run wallet init");
    assert!(stdout.status.success());
}
