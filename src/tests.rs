#[cfg(test)]
mod tests {
    use crate::wallet::Wallet;
    use bdk_wallet::KeychainKind;
    use bitcoin::{Amount, Network};

    use bitcoincore_rpc::{Auth, Client, RpcApi};
    use corepc_node::{Conf, Node};
    use tempfile::Builder;

    #[test]
    fn test_regtest_mine_and_check_balance() {
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = Auth::CookieFile(node.params.cookie_file.clone());

        let client = Client::new(&node.rpc_url(), auth.clone()).expect("rpc client");
        let wallet_name = "test_wallet";
        client
            .create_wallet(wallet_name, None, None, None, None)
            .expect("wallet created and loaded");

        let rpc_url = format!("{}/wallet/{}", node.rpc_url(), wallet_name);
        let client = Client::new(&rpc_url, auth).expect("rpc client");

        let addr = client
            .get_new_address(None, None)
            .expect("new address")
            .assume_checked();
        client.generate_to_address(101, &addr).expect("mine");
        let balances = client.get_balances().expect("balances");

        assert_eq!(balances.mine.trusted.to_btc(), 50.0);
        assert_eq!(balances.mine.immature.to_btc(), 5000.0);
        assert_eq!(balances.mine.untrusted_pending.to_btc(), 0.0);
        assert!(balances.watchonly.is_none());
    }
}
