use bdk_wallet::KeychainKind;
use bitcoin::Network;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use corepc_node::Node;
use tempfile::TempDir;
use url::Url;

use crate::wallet::Wallet;

#[test]
fn test_wallet_creation() {
    let node = Node::from_downloaded().unwrap();
    let node_url: Url = node.rpc_url().parse().expect("valid url");
    let data_dir = TempDir::new().expect("temp dir");
    let mut wallet = Wallet::builder(data_dir.path(), node_url).with_network(Network::Regtest).build();

    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let ans = wallet
        .init(Some(mnemonic.to_string()), None)
        .expect("wallet");
    assert!(ans.created);

    let address = wallet
        .address(KeychainKind::External)
        .expect("valid address");

    assert_eq!(
        address.to_string(),
        "bcrt1p8wpt9v4frpf3tkn0srd97pksgsxc5hs52lafxwru9kgeephvs7rqjeprhg"
    );

    let auth = Auth::CookieFile(node.params.cookie_file.clone());
    let client = Client::new(&node.rpc_url(), auth.clone()).unwrap();
    client.generate_to_address(101, &address).expect("mint");

    // Ensure Core has loaded the relevant blocks before importing descriptors
    std::thread::sleep(std::time::Duration::from_millis(500));

    let wallet_name = wallet.name().expect("wallet name");
    wallet
        .setup_watchonly(&auth, &wallet_name, true)
        .expect("watch-only");
    // wait a bit for rescan to register
    std::thread::sleep(std::time::Duration::from_millis(500));
    let balance = wallet.core_balance(&auth, &wallet_name).expect("balance");
    assert!(balance.to_btc() > 0.0);
}
