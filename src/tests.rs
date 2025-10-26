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
        // Start a corepc-node instance
        let conf = Conf::default();
        let bitcoind = Node::with_conf(corepc_node::downloaded_exe_path().unwrap(), &conf)
            .expect("Failed to start corepc-node instance");
        let rpc_url = bitcoind.rpc_url();
        let auth = Auth::CookieFile(bitcoind.params.cookie_file.clone());

        // Instantiate the application's Wallet
        let temp_dir = Builder::new().prefix("brc721-test").tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let network = Network::Regtest;
        let wallet = Wallet::new(data_dir, network);

        // Initialize local wallet files
        wallet
            .init(None, None)
            .expect("Failed to initialize wallet");
        let wallet_name = wallet
            .generate_wallet_name()
            .expect("Failed to generate wallet name");

        // Set the wallet as watch-only
        wallet
            .setup_watchonly(&rpc_url, &auth, &wallet_name, false)
            .expect("Failed to set up watch-only wallet");

        // Create a new client for the watch-only wallet
        let wallet_rpc_url = format!("{}/{}/{}", bitcoind.rpc_url(), "wallet", wallet_name);
        let client =
            Client::new(&wallet_rpc_url, auth.clone()).expect("Failed to create wallet client");

        // Check initial balance is zero
        let initial_balance = client
            .get_balance(None, None)
            .expect("Failed to get initial wallet balance");
        assert_eq!(initial_balance, Amount::from_btc(0.0).unwrap());

        // Generate an external address
        let address = wallet
            .address(KeychainKind::External)
            .expect("Failed to get external address");

        // Mine 101 blocks to the new address
        bitcoind
            .client
            .generate_to_address(101, &address)
            .expect("Failed to mine blocks to address");

        // Check updated balance
        let updated_balance = client
            .get_balance(None, None)
            .expect("Failed to get updated wallet balance");
        assert_eq!(updated_balance, Amount::from_btc(101.0).unwrap());
    }
}
