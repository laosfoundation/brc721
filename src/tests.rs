#[cfg(test)]
mod tests {
    use bdk_wallet::blockchain::{noop_progress, RpcBlockchain};
    use bdk_wallet::{
        bitcoin::{secp256k1::SecretKey, Network, PrivateKey},
        template::P2Wpkh,
        KeychainKind, Wallet,
    };
    use bitcoincore_rpc::{Auth, Client, RpcApi};
    use corepc_node::Node;

    #[test]
    fn bdk_core_balance_compact() {
        let node = Node::from_downloaded().unwrap();
        let auth = Auth::CookieFile(node.params.cookie_file.clone());
        let core = Client::new(&node.rpc_url(), auth.clone()).unwrap();

        let network = Network::Regtest;
        let k_ext = PrivateKey::new(SecretKey::new(&mut rand::thread_rng()), network);
        let k_int = PrivateKey::new(SecretKey::new(&mut rand::thread_rng()), network);

        let mut wallet = Wallet::create(P2Wpkh(k_ext), P2Wpkh(k_int))
            .network(network)
            .create_wallet_no_persist()
            .expect("wallet");

        let addr_info = wallet.reveal_next_address(KeychainKind::External);
        core.generate_to_address(101, &addr_info.address)
            .expect("mint");

        // 4) sync, otherwise balance is stale
        let blockchain = RpcBlockchain::new(&core, network);
        wallet
            .sync(&blockchain, noop_progress(), None)
            .expect("sync");

        let balance = wallet.balance();
        assert_eq!(balance.total().to_btc(), 1.0);
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
