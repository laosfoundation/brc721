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
        default_value_t = 100usize,
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
        default_value = "http://127.0.0.1:8332",
        value_name = "URL",
        help = "Bitcoin Core RPC URL"
    )]
    pub rpc_url: String,

    #[arg(
        long,
        value_name = "USER",
        default_value = "dev",
        help = "RPC username (user/pass auth)"
    )]
    pub rpc_user: Option<String>,

    #[arg(
        long,
        value_name = "PASS",
        default_value = "dev",
        help = "RPC password (user/pass auth)"
    )]
    pub rpc_pass: Option<String>,
}

pub fn parse() -> Cli {
    Cli::parse()
}
