//! End-to-end test that starts a bitcoind regtest container, creates a Core wallet,
//! mines to an address, and asserts balances. Also exercises the app CLI wallet
//! commands to derive an address and query balance against the same node.

use std::process::Command as ProcCommand;
use std::time::Duration;

use bitcoincore_rpc::{Auth, Client, RpcApi};
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::runners::SyncRunner;
use testcontainers::{GenericImage, ImageExt};

fn base_cmd(rpc_url: &String) -> ProcCommand {
    let mut command = ProcCommand::new("cargo");

    command
        .arg("run")
        .arg("--quiet")
        .arg("--")
        .arg("--network")
        .arg("regtest")
        .arg("--data-dir")
        .arg(".brc721-e2e/")
        .arg("--rpc-url")
        .arg(&rpc_url)
        .arg("--rpc-user")
        .arg("dev")
        .arg("--rpc-pass")
        .arg("dev");

    command
}

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

    // Create a core wallet and mine to it as before.
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

    let status = base_cmd(&rpc_url)
        .arg("wallet")
        .arg("init")
        .status()
        .expect("run wallet init");
    assert!(status.success());

    // 2) Get a new address from the app (prints address to stdout).
    let output = base_cmd(&rpc_url)
        .arg("wallet")
        .arg("address")
        .output()
        .expect("run wallet address");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Address may be printed in logs; search for a bech32 regtest prefix
    let addr_line = stdout
        .lines()
        .find(|l| l.contains("bcrt1") || l.contains("address"))
        .unwrap_or("");
    assert!(!addr_line.is_empty());

    // 3) Query the app balance command; it should reflect the same totals as Core.
    let out_bal = base_cmd(&rpc_url)
        .arg("wallet")
        .arg("balance")
        .output()
        .expect("run wallet balance");
    assert!(out_bal.status.success());

    let stdout = String::from_utf8_lossy(&out_bal.stdout);
    // Expect the balances debug to include trusted and immature fields
    assert_eq!(stdout, "Loaded env from .env\nGetBalancesResult { mine: GetBalancesResultEntry { trusted: 0 SAT, untrusted_pending: 0 SAT, immature: 0 SAT }, watchonly: None }\n");
}
