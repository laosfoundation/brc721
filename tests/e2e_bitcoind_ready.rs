//! End-to-end test that starts a bitcoind regtest container, creates a Core wallet,
//! mines to an address, and asserts balances. Also exercises the app CLI wallet
//! commands to derive an address and query balance against the same node.

use std::process::Command as ProcCommand;
use std::str::FromStr;
use std::time::Duration;

use bitcoin::Address;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::runners::SyncRunner;
use testcontainers::{GenericImage, ImageExt};

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PAYING_ADDRESS: &str = "bcrt1p8wpt9v4frpf3tkn0srd97pksgsxc5hs52lafxwru9kgeephvs7rqjeprhg";

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
fn e2e_balance() {
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

    let stdout = base_cmd(&rpc_url)
        .arg("wallet")
        .arg("init")
        .arg("--mnemonic")
        .arg(MNEMONIC)
        .output()
        .expect("run wallet init");
    assert!(stdout.status.success());

    let stdout = base_cmd(&rpc_url)
        .arg("wallet")
        .arg("address")
        .output()
        .expect("run wallet init");
    println!("{:?}", stdout);

    let addr = Address::from_str(PAYING_ADDRESS)
        .expect("address")
        .assume_checked();
    root_client.generate_to_address(101, &addr).expect("mine");

    // 2) Query the app balance command; it should reflect the same totals as Core.
    let stdout = base_cmd(&rpc_url)
        .arg("wallet")
        .arg("balance")
        .output()
        .expect("run wallet balance");
    assert!(stdout.status.success());

    let stdout = String::from_utf8_lossy(&stdout.stdout);
    // Expect the balances debug to include trusted and immature fields
    assert_eq!(stdout, "Loaded env from .env\nGetBalancesResult { mine: GetBalancesResultEntry { trusted: 5000000000 SAT, untrusted_pending: 0 SAT, immature: 500000000000 SAT }, watchonly: None }\n");
}
