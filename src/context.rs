use anyhow::{Context as AnyhowContext, Result};
use bitcoin::Network;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use std::{net::SocketAddr, path::PathBuf};
use url::Url;

pub struct Context {
    pub network: Network,
    pub data_dir: PathBuf,
    pub rpc_url: Url,
    pub auth: Auth,
    pub confirmations: u64,
    pub batch_size: usize,
    pub start: u64,
    pub log_file: Option<PathBuf>,
    pub reset: bool,
    pub api_listen: SocketAddr,
}

impl Context {
    pub fn from_cli(cli: &crate::cli::Cli) -> Self {
        let auth = match (&cli.rpc_user, &cli.rpc_pass) {
            (Some(user), Some(pass)) => Auth::UserPass(user.clone(), pass.clone()),
            _ => Auth::None,
        };
        let rpc_url = Url::parse(&cli.rpc_url).expect("rpc url");
        let network = detect_network(&rpc_url, &auth).expect("detect network from node");
        let mut data_dir = PathBuf::from(&cli.data_dir);
        let network_dir = network.to_string();
        data_dir.push(network_dir);
        Self {
            network,
            data_dir,
            rpc_url,
            auth,
            confirmations: cli.confirmations,
            batch_size: cli.batch_size,
            start: cli.start,
            log_file: cli.log_file.as_deref().map(PathBuf::from),
            reset: cli.reset,
            api_listen: cli.api_listen,
        }
    }
}

fn detect_network(rpc_url: &Url, auth: &Auth) -> Result<bitcoin::Network> {
    let client = Client::new(rpc_url.as_ref(), auth.clone()).context("create root client")?;
    let info = client
        .get_blockchain_info()
        .context("get_blockchain_info")?;
    Ok(info.chain)
}
