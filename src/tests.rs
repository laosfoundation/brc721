#[cfg(test)]
mod integration {
    use bdk_bitcoind_rpc::{Emitter, NO_EXPECTED_MEMPOOL_TXS};
    use bdk_wallet::{
        bip39::{Language, Mnemonic},
        template::Bip86,
        KeychainKind, Wallet,
    };
    use bitcoin::bip32::Xpriv;
    use bitcoin::{Network, Transaction};
    use bitcoincore_rpc::{Auth, Client, RpcApi};
    use corepc_node::Node;
    use std::sync::Arc;

    fn create_wallet() -> Wallet {
        // Parse the deterministic 12-word BIP39 mnemonic seed phrase.
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).unwrap();
        let network = Network::Regtest;

        // Derive BIP32 master private key from seed.
        let seed = mnemonic.to_seed(String::new()); // empty password
        let master_xprv = Xpriv::new_master(network, &seed).expect("master_key");

        // Initialize the wallet using BIP86 descriptors for both keychains.
        Wallet::create(
            Bip86(master_xprv, KeychainKind::External),
            Bip86(master_xprv, KeychainKind::Internal),
        )
        .network(network)
        .create_wallet_no_persist()
        .expect("wallet")
    }

    #[test]
    fn test_watch_only_wallet_tracking_balance() {
        let mut wallet = create_wallet();

        // Connect to a local Bitcoin regtest node via RPC.
        let node = Node::from_downloaded().unwrap();
        let auth = Auth::CookieFile(node.params.cookie_file.clone());
        let root_client = Client::new(&node.rpc_url(), auth.clone()).unwrap();

        let watch_name = "watch_wallet";
        // Create a blank, watch-only, descriptor-enabled wallet on the node.
        let ans: serde_json::Value = root_client
            .call::<serde_json::Value>(
                "createwallet",
                &[
                    serde_json::json!(watch_name), // Wallet name
                    serde_json::json!(true),       // Disable private keys
                    serde_json::json!(true),       // Blank wallet
                    serde_json::json!(""),         // Passphrase
                    serde_json::json!(false),      // avoid_reuse
                    serde_json::json!(true),       // Descriptors enabled
                ],
            )
            .expect("watch wallet created");
        assert_eq!(ans["name"].as_str().unwrap(), "watch_wallet");
        assert!(ans["warning"].is_null());

        // Create an RPC client for the watch-only wallet.
        let watch_url = format!("{}/wallet/{}", node.rpc_url(), watch_name);
        let watch_client = Client::new(&watch_url, auth).expect("watch client");

        // Import the wallet's external and internal public descriptors into the watch-only wallet.
        let imports = serde_json::json!([
            {
                "desc": wallet.public_descriptor(KeychainKind::External),
                "timestamp": 0,
                "active": true,
                "range": [0,999],
                "internal": false
            },
            {
                "desc": wallet.public_descriptor(KeychainKind::Internal),
                "timestamp": 0,
                "active": true,
                "range": [0,999],
                "internal": true
            }
        ]);
        let ans: serde_json::Value = watch_client
            .call::<serde_json::Value>("importdescriptors", &[imports])
            .expect("import descriptor");

        // Ensure all imports in the response succeeded.
        let arr = ans.as_array().expect("array");
        assert!(
            arr.iter().all(|e| e["success"].as_bool() == Some(true)),
            "{}",
            serde_json::to_string_pretty(&ans).unwrap()
        );

        // Check that balances for the fresh watch-only wallet are zero.
        let balances = watch_client.get_balances().expect("get balances");
        assert_eq!(balances.mine.trusted.to_btc(), 0.0);
        assert_eq!(balances.mine.immature.to_btc(), 0.0);
        assert_eq!(balances.mine.untrusted_pending.to_btc(), 0.0);
        assert!(balances.watchonly.is_none());

        // Generate a fresh address and send 101 blocks to it (mining reward = 50 BTC per block).
        let address = wallet.reveal_next_address(KeychainKind::External);

        root_client
            .generate_to_address(101, &address)
            .expect("mint");

        // Query the balances again; should now reflect the mined funds properly tracked.
        let balances = watch_client.get_balances().expect("get balances");
        assert_eq!(balances.mine.trusted.to_btc(), 50.0); // One mature block reward
        assert_eq!(balances.mine.immature.to_btc(), 5000.0); // 100 immature block rewards
        assert_eq!(balances.mine.untrusted_pending.to_btc(), 0.0);
        assert!(balances.watchonly.is_none());
    }

    #[test]
    fn test_balances_using_local_wallet() {
        let mut wallet = create_wallet();

        // Connect to a local regtest node.
        let node = Node::from_downloaded().unwrap();
        let auth = Auth::CookieFile(node.params.cookie_file.clone());
        let rpc_client = Client::new(&node.rpc_url(), auth.clone()).unwrap();

        // Ensure wallet is empty.
        assert_eq!(wallet.balance().total().to_btc(), 0.0);

        // Get a new address and mine 100 blocks to it.
        let address = wallet.reveal_next_address(KeychainKind::External);
        rpc_client.generate_to_address(100, &address).expect("mint");
        // Balance is still zero (wallet has not synced the blockchain yet).
        assert_eq!(wallet.balance().total().to_btc(), 0.0);

        // Get the current height (tip) and set up a block emitter for syncing.
        let wallet_tip = wallet.latest_checkpoint();
        let mut emitter = Emitter::new(
            &rpc_client,
            wallet_tip.clone(),
            wallet_tip.height(),
            NO_EXPECTED_MEMPOOL_TXS,
        );

        // Apply each new block from the emitter to the wallet.
        while let Some(block) = emitter.next_block().unwrap() {
            wallet
                .apply_block_connected_to(&block.block, block.block_height(), block.connected_to())
                .unwrap()
        }

        // Apply any unconfirmed mempool transactions (should be none in this test).
        let mempool_emissions: Vec<(Arc<Transaction>, u64)> = emitter.mempool().unwrap().update;
        wallet.apply_unconfirmed_txs(mempool_emissions);

        // Balance should now reflect the mined coins (100 blocks x 50 = 5000 BTC).
        assert_eq!(wallet.balance().total().to_btc(), 5000.0);
    }

    #[test]
    fn test_core_node_wallet_mining_and_balance() {
        // Initialize a regtest node and ensure it's running.
        let node = Node::from_downloaded().expect("failed to download node");
        // Use cookie-based authentication from the node's parameters.
        let auth = Auth::CookieFile(node.params.cookie_file.clone());

        // Create a base RPC client for the node (not the wallet yet).
        let client = Client::new(&node.rpc_url(), auth.clone()).expect("rpc client initial");
        let wallet_name = "test_wallet";
        // Create and load a new wallet in the core node.
        client
            .create_wallet(wallet_name, None, None, None, None)
            .expect("wallet created and loaded");

        // Create a new client, pointing specifically to the loaded core node wallet.
        let rpc_url = format!("{}/wallet/{}", node.rpc_url(), wallet_name);
        let client = Client::new(&rpc_url, auth).expect("rpc client for wallet");

        // Generate a new address from the wallet in the core node.
        let addr = client
            .get_new_address(None, None)
            .expect("new address")
            .assume_checked();

        // Mine 101 blocks to the wallet's address to obtain spendable and immature balance.
        client.generate_to_address(101, &addr).expect("mine");
        // Query wallet balances.
        let balances = client.get_balances().expect("get balances");

        // Verify that exactly 50 BTC are immediately spendable.
        assert_eq!(balances.mine.trusted.to_btc(), 50.0);
        // The remainder should be immature block rewards (100 blocks × 50 BTC each).
        assert_eq!(balances.mine.immature.to_btc(), 5000.0);
        // There should be no untrusted pending balance.
        assert_eq!(balances.mine.untrusted_pending.to_btc(), 0.0);
        // Watch-only balances should not be present in this core node wallet.
        assert!(balances.watchonly.is_none());
    }
}
