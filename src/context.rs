use anyhow::{Context as OtherContext, Result};
use bitcoin::Network;
use bitcoincore_rpc::Auth;
use url::Url;

use crate::network::parse_network;

pub struct Context {
    pub network: Network,
    pub data_dir: String,
    pub rpc_url: Url,
    pub auth: Auth,
    pub confirmations: u64,
    pub batch_size: usize,
    pub start: u64,
    pub log_file: Option<String>,
    pub reset: bool,
}

impl Context {
    pub fn try_from_cli(cli: &crate::cli::Cli) -> Result<Self> {
        let network = parse_network(Some(cli.network.clone()));
        let auth = match (&cli.rpc_user, &cli.rpc_pass) {
            (Some(user), Some(pass)) => Auth::UserPass(user.clone(), pass.clone()),
            _ => Auth::None,
        };
        let rpc_url = Url::parse(&cli.rpc_url).context("Failed to parser rpc url")?;
        Ok(Self {
            network,
            data_dir: cli.data_dir.clone(),
            rpc_url,
            auth,
            confirmations: cli.confirmations,
            batch_size: cli.batch_size,
            start: cli.start,
            log_file: cli.log_file.clone(),
            reset: cli.reset,
        })
    }
}
