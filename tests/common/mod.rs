use bitcoincore_rpc::Auth;
use std::process::Command as ProcCommand;
use tempfile::TempDir;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::{Container, ContainerRequest, GenericImage, ImageExt};
use std::str::FromStr;
use bitcoin::Address;

pub fn bitcoind_image() -> ContainerRequest<GenericImage> {
    GenericImage::new("bitcoin/bitcoin", "latest")
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
            "-fallbackfee=0.0002".to_string(),
        ])
}

pub fn auth() -> Auth {
    Auth::UserPass("dev".into(), "dev".into())
}

pub fn rpc_url(container: &Container<GenericImage>) -> String {
    let host_port = container
        .get_host_port_ipv4(18443)
        .expect("mapped port for 18443");

    format!("http://127.0.0.1:{}", host_port)
}

pub fn base_cmd(rpc_url: &String, data_dir: &TempDir) -> ProcCommand {
    let mut command = ProcCommand::new("cargo");

    command
        .arg("run")
        .arg("--quiet")
        .arg("--")
        .arg("--network")
        .arg("regtest")
        .arg("--data-dir")
        .arg(data_dir.path())
        .arg("--rpc-url")
        .arg(rpc_url)
        .arg("--rpc-user")
        .arg("dev")
        .arg("--rpc-pass")
        .arg("dev");

    command
}

pub fn wallet_address(rpc_url: &String, data_dir: &TempDir) -> Address {
    let output = base_cmd(rpc_url, data_dir)
        .arg("wallet")
        .arg("address")
        .output()
        .expect("run wallet address");
    assert!(output.status.success());

    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", out, err);

    // Prefer: the command prints a bare address line on stdout
    for line in combined.lines() {
        let line = line.trim();
        if line.starts_with("bcrt1") {
            return Address::from_str(line).expect("address").assume_checked();
        }
    }

    // Fallback to scanning for bech32 prefix anywhere
    let addr_start = combined.find("bcrt1").expect("address in output");
    let addr_end = combined[addr_start..]
        .find(char::is_whitespace)
        .map(|i| addr_start + i)
        .unwrap_or(combined.len());
    let addr_str = &combined[addr_start..addr_end];

    Address::from_str(addr_str)
        .expect("address")
        .assume_checked()
}
