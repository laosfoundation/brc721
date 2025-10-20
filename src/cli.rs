use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Stream Bitcoin Core blocks and print summaries or detailed scripts",
    long_about = "A simple Rust app that connects to a Bitcoin Core node via RPC and streams blocks."
)]
pub struct Cli {
    #[arg(
        short = 's',
        long = "start",
        default_value_t = 877186u64,
        value_name = "HEIGHT",
        help = "Initial block height to start scanning from when no prior state exists"
    )]
    pub start: u64,

    #[arg(
        short = 'c',
        long,
        default_value_t = 3,
        value_name = "N",
        help = "Only process up to tip - N confirmations"
    )]
    pub confirmations: u64,

    #[arg(
        short = 'b',
        long,
        default_value_t = 1usize,
        value_name = "SIZE",
        help = "Process blocks in batches of SIZE"
    )]
    pub batch_size: usize,

    #[arg(
        long,
        default_value_t = false,
        help = "Reset all persisted state (delete the SQLite database) before starting"
    )]
    pub reset: bool,

    #[arg(
        long,
        default_value = ".brc721/",
        value_name = "DIR",
        help = "Directory to store persistent data"
    )]
    pub data_dir: String,

    #[arg(
        long,
        env = "BITCOIN_NODE_URL",
        default_value = "http://127.0.0.1",
        value_name = "URL",
        help = "Bitcoin node base URL (scheme + host, no port), e.g. http://127.0.0.1"
    )]
    pub node_url: String,

    #[arg(
        long,
        env = "BITCOIN_RPC_USER",
        value_name = "USER",
        default_value = "dev",
        help = "RPC username (user/pass auth)"
    )]
    pub rpc_user: Option<String>,

    #[arg(
        long,
        env = "BITCOIN_RPC_PORT",
        default_value_t = 8332u16,
        value_name = "PORT",
        help = "Bitcoin Core RPC port"
    )]
    pub rpc_port: u16,

    #[arg(
        long,
        env = "BITCOIN_RPC_PASS",
        value_name = "PASS",
        default_value = "dev",
        help = "RPC password (user/pass auth)"
    )]
    pub rpc_pass: Option<String>,

    #[arg(
        long,
        env = "BITCOIN_P2P_PORT",
        value_name = "PORT",
        default_value_t = 8333u16,
        help = "Bitcoin P2P port"
    )]
    pub p2p_port: u16,

    #[arg(
        long,
        value_name = "NETWORK",
        default_value = "mainnet",
        help = "Network for P2P magic: mainnet|testnet|signet|regtest"
    )]
    pub network: String,
}

pub fn parse() -> Cli {
    let _ = dotenvy::dotenv();
    Cli::parse()
}
