use bitcoin::Network;
use bitcoincore_rpc::Auth;

#[derive(Clone)]
pub struct Configuration {
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
