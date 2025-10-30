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
    let wallet = Wallet::new(data_dir.path(), node_url).with_network(Network::Regtest);

    let ans = wallet.init(None, None).expect("wallet");
    assert!(ans.created);

    let address = wallet
        .address(KeychainKind::External)
        .expect("valid address");

    let auth = Auth::CookieFile(node.params.cookie_file.clone());
    let client = Client::new(&node.rpc_url(), auth.clone()).unwrap();
    client.generate_to_address(101, &address).expect("mint");

    let wallet_name = wallet.generate_wallet_name().expect("wallet name");
    let balance = wallet.core_balance(&auth, &wallet_name).expect("balance");
    assert_eq!(balance.to_btc(), 1.0);
}
