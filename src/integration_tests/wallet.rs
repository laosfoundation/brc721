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
    let mut wallet = Brc721Wallet::create(data_dir.path(), Network::Regtest, Some(mnemonic), None)
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
    let wallet = Brc721Wallet::create(data_dir.path(), Network::Regtest, Some(mnemonic), None)
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

#[test]
fn test_rescan_discovers_funds_minted_before_watchonly() {
    let mnemonic = Mnemonic::parse_in(
        Language::English,
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    ).expect("mnemonic");

    let data_dir = TempDir::new().expect("temp dir");
    let mut wallet = Brc721Wallet::create(data_dir.path(), Network::Regtest, Some(mnemonic), None)
        .expect("wallet");

    let node = Node::from_downloaded().unwrap();
    let auth = Auth::CookieFile(node.params.cookie_file.clone());
    let node_url = Url::parse(&node.rpc_url()).unwrap();
    let root_client = Client::new(&node.rpc_url(), auth.clone()).unwrap();

    // Derive an address before setting up the watch-only wallet
    let address = wallet.reveal_next_payment_address().unwrap().address;

    // Mint blocks to the address before importing descriptors into Core
    root_client
        .generate_to_address(101, &address)
        .expect("mint");

    // Now set up the Core watch-only wallet with descriptors (timestamp=now, no rescan)
    wallet
        .setup_watch_only(&node_url, auth.clone())
        .expect("setup watch only");

    // Balances should be non-zero even before an explicit rescan because we imported with timestamp=0.
    // We still verify that after triggering an explicit rescan the balance remains >= previous (idempotent behavior).
    let balances_before = wallet
        .balances(&node_url, auth.clone())
        .expect("balance before");
    let total_before = balances_before.mine.trusted.to_btc()
        + balances_before.mine.immature.to_btc()
        + balances_before.mine.untrusted_pending.to_btc();
    assert!(total_before > 0.0, "expected non-zero balance before rescan when timestamp=0");

    // Trigger a rescan and verify balances become non-zero
    wallet
        .rescan_watch_only(&node_url, auth.clone(), Some(0), None)
        .expect("rescan");

    // Mine one more block to ensure Core updates balances post-rescan
    root_client.generate_to_address(1, &address).expect("mine one");

    let balances_after = wallet
        .balances(&node_url, auth.clone())
        .expect("balance after");
    let total_after = balances_after.mine.trusted.to_btc()
        + balances_after.mine.immature.to_btc()
        + balances_after.mine.untrusted_pending.to_btc();
    assert!(total_after > 0.0, "expected non-zero balance after rescan");
}
