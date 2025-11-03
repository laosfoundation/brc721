use crate::wallet::brc721_wallet::Brc721Wallet;
use bitcoin::{Amount, Network};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use corepc_node::Node;
use tempfile::TempDir;
use url::Url;

#[test]
fn test_send_amount_between_wallets_via_psbt() {
    let node = Node::from_downloaded().unwrap();
    let auth = Auth::CookieFile(node.params.cookie_file.clone());
    let node_url = Url::parse(&node.rpc_url()).unwrap();
    let root_client = Client::new(&node.rpc_url(), auth.clone()).unwrap();

    let data_dir0 = TempDir::new().expect("temp dir");
    let mut wallet0 =
        Brc721Wallet::create(data_dir0.path(), Network::Regtest, None, None).expect("wallet");
    wallet0
        .setup_watch_only(&node_url, auth.clone())
        .expect("setup watch only");

    let address0 = &wallet0.reveal_next_payment_address().unwrap().address;
    root_client
        .generate_to_address(101, address0)
        .expect("mint");

    let data_dir1 = TempDir::new().expect("temp dir");
    let mut wallet1 =
        Brc721Wallet::create(data_dir1.path(), Network::Regtest, None, None).expect("wallet");
    wallet1
        .setup_watch_only(&node_url, auth.clone())
        .expect("setup watch only");

    let address1 = &wallet1.reveal_next_payment_address().unwrap().address;

    let amount = Amount::from_btc(1.0).expect("valid amount");
    let fee = 2.5;
    wallet0
        .send_amount(address1, amount, Some(fee))
        .expect("amount sent");

    let balances = wallet1.balances(&node_url, auth).expect("balances");
    assert_eq!(balances.mine.trusted.to_btc(), 1.0);
    assert_eq!(balances.mine.untrusted_pending.to_btc(), 0.0);
    assert_eq!(balances.mine.immature.to_btc(), 0.0);
}
