use bdk_wallet::bip39::{Language, Mnemonic};
use bitcoin::Network;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use corepc_node::Node;
use tempfile::TempDir;
use url::Url;

use crate::wallet::brc721_wallet::Brc721Wallet;

#[test]
fn test_wallet_creation() {
    let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

    let data_dir = TempDir::new().expect("temp dir");
    let mut wallet = Brc721Wallet::create(
        data_dir.path(),
        Network::Regtest,
        Some(mnemonic),
        "passphrase".to_string(),
    )
    .expect("wallet");

    let node = Node::from_downloaded().unwrap();
    let auth = Auth::CookieFile(node.params.cookie_file.clone());
    let node_url = Url::parse(&node.rpc_url()).unwrap();

    wallet
        .setup_watch_only(&node_url, auth.clone())
        .expect("setup watch only");

    let root_client = Client::new(&node.rpc_url(), auth.clone()).unwrap();

    let address = &wallet.reveal_next_payment_address().unwrap().address;

    root_client.generate_to_address(101, address).expect("mint");

    let balances = wallet.balances(&node_url, auth.clone()).expect("balance");
    assert_eq!(balances.mine.trusted.to_btc(), 50.0); // One mature block reward
    assert_eq!(balances.mine.immature.to_btc(), 5000.0); // 100 immature block rewards
    assert_eq!(balances.mine.untrusted_pending.to_btc(), 0.0);
    assert!(balances.watchonly.is_none());
}

#[test]
fn test_setup_watch_only_idempotent() {
    let mnemonic = Mnemonic::parse_in(
        Language::English,
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    ).expect("mnemonic");

    let data_dir = TempDir::new().expect("temp dir");
    let wallet = Brc721Wallet::create(
        data_dir.path(),
        Network::Regtest,
        Some(mnemonic),
        "passphrase".to_string(),
    )
    .expect("wallet");

    let node = Node::from_downloaded().unwrap();
    let auth = Auth::CookieFile(node.params.cookie_file.clone());
    let node_url = Url::parse(&node.rpc_url()).unwrap();

    // First call to setup_watch_only
    wallet
        .setup_watch_only(&node_url, auth.clone())
        .expect("first setup watch only");
    // Second call should also succeed and not change state or error
    wallet
        .setup_watch_only(&node_url, auth.clone())
        .expect("idempotent setup watch only");
}
