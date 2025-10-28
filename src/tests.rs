#[cfg(test)]
mod tests {
    use bdk_wallet::{
        bip39::{Language, Mnemonic},
        template::Bip86,
        KeychainKind, Wallet,
    };
    use bitcoin::bip32::Xpriv;
    use bitcoin::Network;
    use bitcoincore_rpc::{Auth, Client, RpcApi};
    use corepc_node::Node;

    #[test]
    fn my_test() {
        // the seed
        let mnemonic = Mnemonic::parse_in(Language::English,
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    ).unwrap();
        let network = Network::Regtest;
        let password = String::new();
        let seed = mnemonic.to_seed(password);
        let xprv = Xpriv::new_master(network, &seed).expect("master_key");
        let external = Bip86(xprv, KeychainKind::External).build(network);
    }

    #[test]
    fn watch_only_wallet_balance_minimal_3() {
        use bitcoincore_rpc::{Auth, Client, RpcApi};
        use corepc_node::Node;

        // 1) start node (downloaded regtest node)
        let node = Node::from_downloaded().expect("failed to download node");
        let auth = Auth::CookieFile(node.params.cookie_file.clone());
        let core = Client::new(&node.rpc_url(), auth.clone()).expect("rpc client");

        // 2) create a wallet that will receive/mine coins ("funding")
        core.create_wallet("funding", None, None, None, None)
            .expect("create funding wallet");
        let funding_rpc = Client::new(
            &format!("{}/wallet/{}", node.rpc_url(), "funding"),
            auth.clone(),
        )
        .expect("funding wallet rpc");

        // 3) get an address from funding wallet and mine coinbase to it (101 blocks -> mature)
        let addr = funding_rpc
            .get_new_address(None, None)
            .expect("new addr")
            .assume_checked();
        core.generate_to_address(101, &addr).expect("mine to addr");

        // 4) create watch-only wallet (disable private keys)
        core.create_wallet("watch", Some(true), None, None, None)
            .expect("create watch wallet");
        let watch_rpc = Client::new(
            &format!("{}/wallet/{}", node.rpc_url(), "watch"),
            auth.clone(),
        )
        .expect("watch wallet rpc");

        // 5) import the funding address into the watch wallet as watch-only and rescan so it sees the funds
        //    (rescan can take some time; here we just call it — regtest is tiny so it's instant)
        watch_rpc
            .import_address(&addr, None, Some(true))
            .expect("import address");

        // 6) query the watch-only wallet balance
        //    Use get_balance(None, None) which returns a bitcoin::Amount
        let balance = watch_rpc.get_balance(None, None).expect("get balance");

        // Minimal assertion: balance must be greater than zero
        assert!(balance.to_sat() > 0, "watch-only wallet should have funds");

        // Print for human debugging
        eprintln!("watch-only wallet balance (satoshis): {}", balance.to_sat());
    }

    #[test]
    fn watch_only_wallet_balance_minimal() {
        use bitcoincore_rpc::{Auth, Client, RpcApi};
        use corepc_node::Node;

        // 1) boot regtest node
        let node = Node::from_downloaded().expect("node");
        let auth = Auth::CookieFile(node.params.cookie_file.clone());
        let core = Client::new(&node.rpc_url(), auth.clone()).expect("rpc");

        // 2) funding wallet (has privkeys) -> gets mined coins
        core.create_wallet("funding", None, None, None, None)
            .expect("funding wallet");
        let frpc = Client::new(
            &format!("{}/wallet/{}", node.rpc_url(), "funding"),
            auth.clone(),
        )
        .expect("funding rpc");
        let mine_addr = frpc
            .get_new_address(None, None)
            .expect("addr")
            .assume_checked();
        core.generate_to_address(101, &mine_addr).expect("mine 101");

        // 3) watch-only wallet (no privkeys)
        //    createwallet name disable_private_keys=true
        core.create_wallet("watch", Some(true), None, None, None)
            .expect("watch wallet");
        let wrpc = Client::new(&format!("{}/wallet/{}", node.rpc_url(), "watch"), auth)
            .expect("watch rpc");

        // 4) import address and rescan so it sees past funds
        wrpc.import_address(&mine_addr, None, Some(true))
            .expect("import+rescan");

        // 5) check balance
        let bal = wrpc.get_balance(None, None).expect("balance");
        assert!(bal.to_sat() > 0, "watch-only balance should be > 0");
        eprintln!("watch-only balance (sats): {}", bal.to_sat());
    }

    #[test]
    fn watch_only_balance_descriptor_wallet_minimal_4() {
        use bitcoincore_rpc::{Auth, Client, RpcApi};
        use corepc_node::Node;
        use serde_json::json;

        let node = Node::from_downloaded().unwrap();
        let auth = Auth::CookieFile(node.params.cookie_file.clone());
        let core = Client::new(&node.rpc_url(), auth.clone()).unwrap();

        // Wallet with keys just to get an address to watch
        core.create_wallet("funding", None, None, None, None)
            .unwrap();
        let frpc = Client::new(
            &format!("{}/wallet/{}", node.rpc_url(), "funding"),
            auth.clone(),
        )
        .unwrap();
        let addr = frpc.get_new_address(None, None).unwrap().assume_checked();

        // Watch-only, descriptor wallet
        core.create_wallet("watch", Some(true), None, None, None)
            .unwrap();
        let wrpc = Client::new(
            &format!("{}/wallet/{}", node.rpc_url(), "watch"),
            auth.clone(),
        )
        .unwrap();

        // Import the single address via descriptor
        let req = json!([{
            "desc": format!("addr({})", addr),
            "timestamp": "now",
            "active": true
        }]);
        wrpc.call::<serde_json::Value>("importdescriptors", &[req])
            .unwrap();

        // Fund after import so no rescan needed
        core.generate_to_address(101, &addr).unwrap();

        // Check balance
        let bal = wrpc.get_balance(None, None).unwrap();
        assert!(bal.to_sat() > 0, "watch-only balance should be > 0");
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
