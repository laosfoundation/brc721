#[cfg(test)]
mod tests {
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

        let balance = wallet.balance();
        assert_eq!(balance.total().to_btc(), 1.0);
    }

    #[test]
    fn test_regtest_mine_and_check_balance() {
        let node = Node::from_downloaded().expect("failed to download node");
        let auth = Auth::CookieFile(node.params.cookie_file.clone());

        let client = Client::new(&node.rpc_url(), auth.clone()).expect("rpc client initial");
        let wallet_name = "test_wallet";
        client
            .create_wallet(wallet_name, None, None, None, None)
            .expect("wallet created and loaded");

        let rpc_url = format!("{}/wallet/{}", node.rpc_url(), wallet_name);
        let client = Client::new(&rpc_url, auth).expect("rpc client for wallet");

        let addr = client
            .get_new_address(None, None)
            .expect("new address")
            .assume_checked();

        client.generate_to_address(101, &addr).expect("mine");
        let balances = client.get_balances().expect("get balances");

        assert_eq!(balances.mine.trusted.to_btc(), 50.0);
        assert_eq!(balances.mine.immature.to_btc(), 5000.0);
        assert_eq!(balances.mine.untrusted_pending.to_btc(), 0.0);
        assert!(balances.watchonly.is_none());
    }
}
