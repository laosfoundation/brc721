#[cfg(test)]
mod tests {
    use crate::wallet::Wallet;
    use bdk_wallet::KeychainKind;
    use bitcoin::{Amount, Network};

    use bitcoincore_rpc::Auth;
    use corepc_node::{Conf, Node};
    use tempfile::Builder;

    #[test]
    fn test_wallet_balance() {
        // 1. Start a corepc-node instance
        let conf = Conf::default();
        let bitcoind = Node::with_conf(corepc_node::downloaded_exe_path().unwrap(), &conf).unwrap();

        // 2. Instantiate the application's Wallet
        let temp_dir = Builder::new().prefix("brc721-test").tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let network = Network::Regtest;
        let wallet = Wallet::new(data_dir, network);

        // 3. Call wallet.init(...) to create the local wallet files
        wallet.init(None, None).unwrap();
        let wallet_name = wallet.generate_wallet_name().unwrap();
        let auth = Auth::CookieFile(bitcoind.params.cookie_file.clone());
        wallet
            .setup_watchonly(&bitcoind.rpc_url(), &auth, &wallet_name, false)
            .unwrap();

        // 4. Call wallet.address(...) to get a new receiving address
        let address = wallet.address(KeychainKind::External).unwrap();

        // 5. Use the corepc-node client to mine 101 blocks to this new address
        bitcoind.client.generate_to_address(101, &address).unwrap();

        // 6. Call wallet.core_balance(...) to get the balance
        let balance = wallet
            .core_balance(&bitcoind.rpc_url(), &auth, &wallet_name)
            .unwrap();

        // 7. Assert that the balance is the expected amount
        assert_eq!(balance, Amount::from_btc(50.0).unwrap());
    }
}
