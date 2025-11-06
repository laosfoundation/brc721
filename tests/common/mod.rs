use bitcoincore_rpc::Auth;
use std::process::Command as ProcCommand;
use tempfile::TempDir;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::{Container, ContainerRequest, GenericImage, ImageExt};

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
