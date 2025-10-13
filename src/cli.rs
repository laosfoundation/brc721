use clap::{Parser, Subcommand};

#[derive(Subcommand, Debug)]
pub enum Command {
    WalletInit {
        #[arg(long)]
        name: String,
    },
    WalletNewAddress {
        #[arg(long)]
        name: String,
    },
    WalletBalance {
        #[arg(long)]
        name: String,
    },
    CollectionCreate {
        #[arg(long, value_name = "HEX20")]
        laos_hex: String,
        #[arg(long, default_value_t = false)]
        rebaseable: bool,
        #[arg(long)]
        fee_rate: Option<f64>,
        #[arg(long)]
        name: String,
    },
}

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Stream Bitcoin Core blocks and print summaries or detailed scripts",
    long_about = "A simple Rust app that connects to a Bitcoin Core node via RPC and streams blocks.\n\nEnvironment:\n  BITCOIN_RPC_URL       RPC URL (default http://127.0.0.1:8332)\n  BITCOIN_RPC_USER      RPC username\n  BITCOIN_RPC_PASS      RPC password\n  BITCOIN_RPC_COOKIE    Path to cookie file\n"
)]
pub struct Cli {
    #[arg(short, long, help = "Print transaction scripts and details")]
    pub debug: bool,

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

    #[command(subcommand)]
    pub command: Option<Command>,
}

pub fn parse() -> Cli {
    Cli::parse()
}
