use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Stream Bitcoin Core blocks and print summaries or detailed scripts",
    long_about = "A simple Rust app that connects to a Bitcoin Core node via RPC and streams blocks.\n\nEnvironment:\n  BITCOIN_RPC_URL       RPC URL (default http://127.0.0.1:8332)\n  BITCOIN_RPC_USER      RPC username\n  BITCOIN_RPC_PASS      RPC password\n  BITCOIN_RPC_COOKIE    Path to cookie file\n",
)]
pub struct Cli {
    #[arg(short, long, help = "Print transaction scripts and details")]
    pub debug: bool,

    #[arg(
        short = 'c',
        long,
        default_value_t = 0,
        value_name = "N",
        help = "Only process up to tip - N confirmations"
    )]
    pub confirmations: u64,
}

pub fn parse() -> Cli {
    Cli::parse()
}
