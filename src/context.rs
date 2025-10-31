use bitcoin::Network;
use bitcoincore_rpc::Auth;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

use crate::network::parse_network;

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
}

impl Context {
    pub fn from_cli(cli: &crate::cli::Cli) -> Self {
        let network = parse_network(Some(cli.network.clone()));
        let auth = match (&cli.rpc_user, &cli.rpc_pass) {
            (Some(user), Some(pass)) => Auth::UserPass(user.clone(), pass.clone()),
            _ => Auth::None,
        };
        let rpc_url = Url::parse(&cli.rpc_url).expect("rpc url");
        Self {
            network,
            data_dir: PathBuf::from(&cli.data_dir),
            rpc_url,
            auth,
            confirmations: cli.confirmations,
            batch_size: cli.batch_size,
            start: cli.start,
            log_file: cli.log_file.as_deref().map(PathBuf::from),
            reset: cli.reset,
        }
    }
}
