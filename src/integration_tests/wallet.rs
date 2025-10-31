use bdk_wallet::bip39::{Language, Mnemonic};
use bitcoin::Network;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use corepc_node::Node;
use tempfile::TempDir;

use crate::wallet::brc721_wallet::Brc721Wallet;

#[test]
fn test_wallet_creation() {
    let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

    let data_dir = TempDir::new().expect("temp dir");
    let mut wallet =
        Brc721Wallet::create(data_dir.path(), Network::Regtest, mnemonic).expect("wallet");

    let node = Node::from_downloaded().unwrap();
    let auth = Auth::CookieFile(node.params.cookie_file.clone());
    let root_client = Client::new(&node.rpc_url(), auth.clone()).unwrap();

    let address = wallet.reveal_next_payment_address();
    root_client
        .generate_to_address(101, &address)
        .expect("mint");

    let balance = wallet.balance();
}
