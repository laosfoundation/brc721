use tempfile::TempDir;
use testcontainers::runners::SyncRunner;

mod common;

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PASSPHRASE: &str = "passphrase";

#[test]
fn e2e_wallet_recreate_after_local_data_loss() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);

    let data_dir_original = TempDir::new().expect("temp dir");
    let init_first = common::base_cmd(&rpc_url, &data_dir_original)
        .arg("wallet")
        .arg("init")
        .arg("--passphrase")
        .arg(PASSPHRASE)
        .arg("--mnemonic")
        .arg(MNEMONIC)
        .output()
        .expect("first wallet init");
    assert!(
        init_first.status.success(),
        "first wallet init stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init_first.stdout),
        String::from_utf8_lossy(&init_first.stderr)
    );

    let first_address = common::wallet_address(&rpc_url, &data_dir_original);
    let original_path = data_dir_original.path().to_path_buf();
    drop(data_dir_original);
    assert!(
        !original_path.exists(),
        "original data dir should be removed to simulate data loss"
    );

    let data_dir_recreated = TempDir::new().expect("temp dir");
    let init_second = common::base_cmd(&rpc_url, &data_dir_recreated)
        .arg("wallet")
        .arg("init")
        .arg("--passphrase")
        .arg(PASSPHRASE)
        .arg("--mnemonic")
        .arg(MNEMONIC)
        .output()
        .expect("second wallet init");
    assert!(
        init_second.status.success(),
        "second wallet init stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init_second.stdout),
        String::from_utf8_lossy(&init_second.stderr)
    );

    let recreated_address = common::wallet_address(&rpc_url, &data_dir_recreated);
    assert_eq!(
        first_address, recreated_address,
        "Wallet recreated from same mnemonic should derive the same first address"
    );
}
