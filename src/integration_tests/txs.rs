use crate::wallet::brc721_wallet::Brc721Wallet;
use bitcoin::{Amount, Network};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use corepc_node::Node;
use tempfile::TempDir;
use url::Url;

#[test]
fn test_send_amount_between_wallets_via_psbt() {
    // Start a regtest node and set up RPC authentication
    let node = Node::from_downloaded().unwrap();
    let auth = Auth::CookieFile(node.params.cookie_file.clone());
    let node_url = Url::parse(&node.rpc_url()).unwrap();
    let root_client = Client::new(&node.rpc_url(), auth.clone()).unwrap();

    // Create first temporary wallet directory and initialize Brc721Wallet
    let data_dir0 = TempDir::new().expect("temp dir");
    let mut wallet0 = Brc721Wallet::create(
        data_dir0.path(),
        Network::Regtest,
        None,
        Some("passphrase".to_string()),
    )
    .expect("wallet");
    wallet0
        .setup_watch_only(&node_url, auth.clone())
        .expect("setup watch only");

    // Get the payment address for wallet0
    let address0 = &wallet0.reveal_next_payment_address().unwrap().address;
    // Fund wallet0 with 101 blocks (skips coinbase maturity for 1 block)
    root_client
        .generate_to_address(101, address0)
        .expect("mint");

    // Check wallet0 balance (should have 50 BTC mature, 0 untrusted, 5000 BTC immature)
    let balances0 = wallet0.balances(&node_url, auth.clone()).expect("balances");
    assert_eq!(balances0.mine.trusted.to_btc(), 50.0);
    assert_eq!(balances0.mine.untrusted_pending.to_btc(), 0.0);
    assert_eq!(balances0.mine.immature.to_btc(), 5000.0);

    let passphrase = Some("passphrase".to_string());

    // Create second temporary wallet directory and initialize Brc721Wallet
    let data_dir1 = TempDir::new().expect("temp dir");
    let mut wallet1 =
        Brc721Wallet::create(data_dir1.path(), Network::Regtest, None, passphrase.clone())
            .expect("wallet");
    wallet1
        .setup_watch_only(&node_url, auth.clone())
        .expect("setup watch only");

    // Get the payment address for wallet1
    let address1 = &wallet1.reveal_next_payment_address().unwrap().address;

    // Set the send amount and fee (BTC)
    let amount = Amount::from_btc(1.0).expect("valid amount");
    let fee = 2.5;
    // Send from wallet0 to wallet1 via PSBT flow
    wallet0
        .send_amount(
            &node_url,
            auth.clone(),
            address1,
            amount,
            Some(fee),
            passphrase,
        )
        .expect("amount sent");

    // Mine a block to confirm the transaction so funds appear as trusted in wallet1
    root_client
        .generate_to_address(1, address0)
        .expect("mine confirm");

    // Check wallet1 balance after transaction; account for any fee variance
    let balances1 = wallet1.balances(&node_url, auth).expect("balances");
    assert_eq!(balances1.mine.trusted.to_btc(), 0.99999612);
    assert_eq!(balances1.mine.untrusted_pending.to_btc(), 0.0);
    assert_eq!(balances1.mine.immature.to_btc(), 0.0);
}
