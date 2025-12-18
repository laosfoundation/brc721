use crate::{
    types::{Brc721Output, Brc721Payload, RegisterCollectionData},
    wallet::brc721_wallet::Brc721Wallet,
};
use age::secrecy::SecretString;
use bdk_wallet::bip39::{Language, Mnemonic};
use bitcoin::{Amount, Network};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use corepc_node::Node;
use ethereum_types::H160;
use tempfile::TempDir;
use url::Url;

#[test]
fn test_build_tx_creates_signed_tx_with_custom_output() {
    let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");
    let data_dir = TempDir::new().expect("temp dir");
    let node = corepc_node::Node::from_downloaded().unwrap();
    let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
    let node_url = url::Url::parse(&node.rpc_url()).unwrap();
    let mut wallet = Brc721Wallet::create(
        &data_dir,
        Network::Regtest,
        mnemonic,
        SecretString::from("passphrase".to_string()),
        &node_url,
        auth.clone(),
    )
    .expect("wallet");
    wallet.setup_watch_only().expect("setup watch only");
    let address = wallet
        .reveal_next_payment_address()
        .expect("address")
        .address;
    let client = bitcoincore_rpc::Client::new(&node.rpc_url(), auth.clone()).unwrap();
    client.generate_to_address(101, &address).expect("mint");
    let output = Brc721Output::new(Brc721Payload::RegisterCollection(RegisterCollectionData {
        evm_collection_address: H160::default(),
        rebaseable: false,
    }))
    .into_txout()
    .unwrap();
    assert_eq!(
        output.script_pubkey.to_string(),
        "OP_RETURN OP_PUSHNUM_15 OP_PUSHBYTES_22 00000000000000000000000000000000000000000000"
    );
    let tx = wallet
        .build_tx(
            output,
            Some(1.5),
            SecretString::from("passphrase".to_string()),
        )
        .expect("build tx");
    assert!(!tx.input.is_empty(), "built tx must have inputs");
    assert!(!tx.output.is_empty(), "built tx must have outputs");
    assert_eq!(
        tx.output[0].script_pubkey.to_string(),
        "OP_RETURN OP_PUSHNUM_15 OP_PUSHBYTES_22 00000000000000000000000000000000000000000000"
    );
    let txid = wallet.broadcast(&tx).expect("broadcast");
    assert_ne!(txid.to_string(), String::new());
}

#[test]
fn test_send_amount_between_wallets_via_psbt() {
    let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");
    let mnemonic1 = Mnemonic::parse_in(
        Language::English,
        "core basket honey spike miracle appear such orphan hand wire profit dumb",
    )
    .expect("mnemonic");
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
        mnemonic,
        SecretString::from("passphrase".to_string()),
        &node_url,
        auth.clone(),
    )
    .expect("wallet");
    wallet0.setup_watch_only().expect("setup watch only");

    // Get the payment address for wallet0
    let address0 = &wallet0.reveal_next_payment_address().unwrap().address;
    // Fund wallet0 with 101 blocks (skips coinbase maturity for 1 block)
    root_client
        .generate_to_address(101, address0)
        .expect("mint");

    // Check wallet0 balance (should have 50 BTC mature, 0 untrusted, 5000 BTC immature)
    let balances0 = wallet0.balances().expect("balances");
    assert_eq!(balances0.mine.trusted.to_btc(), 50.0);
    assert_eq!(balances0.mine.untrusted_pending.to_btc(), 0.0);
    assert_eq!(balances0.mine.immature.to_btc(), 5000.0);

    let passphrase = "passphrase".to_string();

    // Create second temporary wallet directory and initialize Brc721Wallet
    let data_dir1 = TempDir::new().expect("temp dir");
    let mut wallet1 = Brc721Wallet::create(
        data_dir1.path(),
        Network::Regtest,
        mnemonic1,
        SecretString::from(passphrase.clone()),
        &node_url,
        auth.clone(),
    )
    .expect("wallet");
    wallet1.setup_watch_only().expect("setup watch only");

    // Get the payment address for wallet1
    let address1 = &wallet1.reveal_next_payment_address().unwrap().address;

    // Set the send amount and fee (BTC)
    let amount = Amount::from_btc(1.0).expect("valid amount");
    let fee = 2.5;
    // Send from wallet0 to wallet1 via PSBT flow
    let tx = wallet0
        .build_payment_tx(address1, amount, Some(fee), SecretString::from(passphrase))
        .expect("build payment tx");
    wallet0.broadcast(&tx).expect("broadcast");

    // Mine a block to confirm the transaction so funds appear as trusted in wallet1
    root_client
        .generate_to_address(1, address0)
        .expect("mine confirm");

    // Check wallet1 balance after transaction; account for any fee variance
    let balances1 = wallet1.balances().expect("balances");
    assert_eq!(balances1.mine.trusted.to_btc(), 1.0);
    assert_eq!(balances1.mine.untrusted_pending.to_btc(), 0.0);
    assert_eq!(balances1.mine.immature.to_btc(), 0.0);
}
