use clap::Parser;
use std::env;

use crate::cli::command::Command;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Stream Bitcoin Core blocks and print summaries or detailed scripts",
    long_about = "A simple Rust app that connects to a Bitcoin Core node via RPC and streams blocks.",
    subcommand_required = false,
    arg_required_else_help = false
)]
pub struct Cli {
    #[arg(
        short = 's',
        long = "start",
        env = "BRC721_START_BLOCK",
        default_value_t = 923580u64,
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
        env = "BITCOIN_RPC_URL",
        default_value = "http://127.0.0.1:8332",
        value_name = "URL",
        help = "Bitcoin Core RPC URL"
    )]
    pub rpc_url: String,

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
        env = "BITCOIN_RPC_PASS",
        value_name = "PASS",
        default_value = "dev",
        help = "RPC password (user/pass auth)"
    )]
    pub rpc_pass: Option<String>,

    #[arg(
        long = "log-file",
        env = "BRC721_LOG_FILE",
        value_name = "PATH",
        help = "Write logs to PATH (in addition to stderr)"
    )]
    pub log_file: Option<String>,

    #[arg(
        long = "api-listen",
        env = "BRC721_API_LISTEN",
        value_name = "ADDR",
        default_value = "127.0.0.1:8083",
        help = "REST API listen address (host:port)"
    )]
    pub api_listen: std::net::SocketAddr,

    #[command(subcommand)]
    pub cmd: Option<Command>,
}

pub fn parse() -> Cli {
    let dotenv_path = env::var("DOTENV_PATH").unwrap_or(".env".into());
    dotenvy::from_filename(&dotenv_path).ok();

    println!("Loaded env from {}", dotenv_path);
    Cli::parse()
}
