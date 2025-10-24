use bitcoin::Network;
use bitcoincore_rpc::Auth;

use crate::network::parse_network;

pub struct Context {
    pub network: Network,
    pub data_dir: String,
    pub rpc_url: String,
    pub auth: Auth,
    pub confirmations: u64,
    pub batch_size: usize,
    pub start: u64,
    pub log_file: Option<String>,
    pub reset: bool,
}

impl Context {
    pub fn from_cli(cli: &crate::cli::Cli) -> Self {
        let network = parse_network(Some(cli.network.clone()));
        let auth = match (&cli.rpc_user, &cli.rpc_pass) {
            (Some(user), Some(pass)) => Auth::UserPass(user.clone(), pass.clone()),
            _ => Auth::None,
        };
        Self {
            network,
            data_dir: cli.data_dir.clone(),
            rpc_url: cli.rpc_url.clone(),
            auth,
            confirmations: cli.confirmations,
            batch_size: cli.batch_size,
            start: cli.start,
            log_file: cli.log_file.clone(),
            reset: cli.reset,
        }
    }
}
