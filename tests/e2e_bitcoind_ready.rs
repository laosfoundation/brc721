mod e2e {
    use std::time::Duration;

    use bitcoincore_rpc::{Auth, Client, RpcApi};
    use testcontainers::core::{ContainerPort, WaitFor};
    use testcontainers::runners::SyncRunner;
    use testcontainers::{GenericImage, ImageExt};

    #[test]
    fn bitcoind_wallet_mine_and_balance() {
        let image = GenericImage::new("bitcoin/bitcoin", "latest")
            .with_wait_for(WaitFor::message_on_stdout("Binding RPC on address"))
            .with_wait_for(WaitFor::message_on_stdout("init message: Done loading"))
            .with_exposed_port(ContainerPort::Tcp(18443))
            .with_cmd(vec![
                "bitcoind".to_string(),
                "-regtest=1".to_string(),
                "-server=1".to_string(),
                "-txindex=1".to_string(),
                "-rpcbind=0.0.0.0".to_string(),
                "-rpcallowip=0.0.0.0/0".to_string(),
                "-rpcuser=dev".to_string(),
                "-rpcpassword=dev".to_string(),
            ]);

        let container = image.start().expect("start bitcoind container");

        let host_port = container
            .get_host_port_ipv4(18443)
            .expect("mapped port for 18443");

        let rpc_url = format!("http://127.0.0.1:{}", host_port);
        let auth = Auth::UserPass("dev".into(), "dev".into());

        let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

        let mut attempts = 0;
        loop {
            match root_client.get_block_count() {
                Ok(_height) => break,
                Err(_e) if attempts < 60 => {
                    std::thread::sleep(Duration::from_secs(1));
                    attempts += 1;
                }
                Err(e) => panic!("RPC not ready: {e}"),
            }
        }

        let wallet_name = "test_wallet";
        root_client
            .create_wallet(wallet_name, None, None, None, None)
            .expect("wallet created and loaded");

        let wallet_url = format!("{}/wallet/{}", rpc_url, wallet_name);
        let wallet_client = Client::new(&wallet_url, auth.clone()).expect("rpc client for wallet");

        let addr = wallet_client
            .get_new_address(None, None)
            .expect("new address")
            .assume_checked();

        root_client.generate_to_address(101, &addr).expect("mine");

        let balances = wallet_client.get_balances().expect("get balances");

        assert_eq!(balances.mine.trusted.to_btc(), 50.0);
        assert_eq!(balances.mine.immature.to_btc(), 5000.0);
        assert_eq!(balances.mine.untrusted_pending.to_btc(), 0.0);
        assert!(balances.watchonly.is_none());
    }
}
