use crate::configuration::Configuration;

pub struct Context {
    pub config: Configuration,
}

impl Context {
    pub fn from_cli(cli: &crate::cli::Cli) -> Self {
        let cfg = Configuration {
            network: crate::network::parse_network(Some(cli.network.clone())),
            data_dir: cli.data_dir.clone(),
            rpc_url: cli.rpc_url.clone(),
            auth: match (&cli.rpc_user, &cli.rpc_pass) {
                (Some(user), Some(pass)) => bitcoincore_rpc::Auth::UserPass(user.clone(), pass.clone()),
                _ => bitcoincore_rpc::Auth::None,
            },
            confirmations: cli.confirmations,
            batch_size: cli.batch_size,
            start: cli.start,
            log_file: cli.log_file.clone(),
            reset: cli.reset,
        };
        Self { config: cfg }
    }
}
