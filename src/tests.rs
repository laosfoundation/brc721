#[cfg(test)]
mod tests {
    use crate::wallet::Wallet;
    use bdk_wallet::KeychainKind;
    use bitcoin::{Amount, Network};

    use bitcoincore_rpc::{Auth, Client, RpcApi};
    use corepc_node::{Conf, Node};
    use tempfile::Builder;

    #[test]
    fn test_watchonly_balance() {
        // 1. Start a corepc-node instance
        let conf = Conf::default();
        let bitcoind = Node::with_conf(corepc_node::downloaded_exe_path().unwrap(), &conf).unwrap();
        let rpc_url = bitcoind.rpc_url();
        let auth = Auth::CookieFile(bitcoind.params.cookie_file.clone());

        // 2. Instantiate the application's Wallet
        let temp_dir = Builder::new().prefix("brc721-test").tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let network = Network::Regtest;
        let wallet = Wallet::new(data_dir, network);

        // 3. Call wallet.init(...) to create the local wallet files
        wallet.init(None, None).unwrap();
        let wallet_name = wallet.generate_wallet_name().unwrap();

        // 4. Set the watch_only
        wallet
            .setup_watchonly(&rpc_url, &auth, &wallet_name, false)
            .unwrap();

        // get the balance
        let rpc_url = format!("{}/{}/{}", bitcoind.rpc_url(), "wallet", wallet_name);
        let client = Client::new(&rpc_url, auth.clone()).unwrap();
        let balance = client.get_balance(None, None).unwrap();
        assert_eq!(balance, Amount::from_btc(0.0).unwrap());

        let address = wallet.address(KeychainKind::External).unwrap();

        // 5. Use the corepc-node client to mine 101 blocks to this new address
        bitcoind.client.generate_to_address(101, &address).unwrap();

        let balance = client.get_balance(None, None).unwrap();
        assert_eq!(balance, Amount::from_btc(101.0).unwrap());
    }
}
