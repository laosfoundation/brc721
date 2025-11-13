use clap::{Parser, Subcommand};
use ethereum_types::H160;
use std::env;

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

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    #[command(
        about = "Wallet management commands",
        long_about = "Create or import a mnemonic and manage a corresponding Bitcoin Core watch-only wallet, derive addresses, and inspect balances."
    )]
    Wallet {
        #[command(subcommand)]
        cmd: WalletCmd,
    },
    #[command(
        about = "Transaction-related commands",
        long_about = "Build and submit protocol transactions, such as registering BRC-721 collections."
    )]
    Tx {
        #[command(subcommand)]
        cmd: TxCmd,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum WalletCmd {
    #[command(
        about = "Initialize wallet and Core watch-only wallet",
        long_about = "Create or import a BIP39 mnemonic and set up a corresponding Bitcoin Core watch-only wallet with descriptors. Optionally import an existing mnemonic and passphrase, set a custom Core wallet name, and trigger a full rescan."
    )]
    Init {
        #[arg(
            long,
            value_name = "MNEMONIC",
            help = "Existing 12-24 words mnemonic",
            required = false
        )]
        mnemonic: Option<String>,
        #[arg(
            long,
            value_name = "PASSPHRASE",
            help = "Passphrase for the mnemonic",
            required = false
        )]
        passphrase: Option<String>,
    },
    #[command(
        about = "Get a new receive address",
        long_about = "Advance derivation and display the next unused receive address (state is persisted)."
    )]
    Address,
    #[command(
        about = "Show wallet balance",
        long_about = "Display confirmed and unconfirmed wallet balances as tracked via the Core watch-only wallet and local index."
    )]
    Balance,
    #[command(
        about = "Trigger a Core wallet rescan",
        long_about = "Ask Bitcoin Core to rescan the blockchain for the watch-only wallet's descriptors."
    )]
    Rescan,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TxCmd {
    #[command(
        about = "Register a BRC-721 collection",
        long_about = "Create and broadcast a transaction that registers a BRC-721 collection, linking a 20-byte EVM (H160) address. Optionally mark the collection as rebaseable and set a custom fee rate (sat/vB)."
    )]
    RegisterCollection {
        #[arg(
            long = "collection-address",
            value_name = "H160",
            help = "20-byte EVM collection address (H160)",
            required = true
        )]
        collection_address: H160,
        #[arg(
            long,
            default_value_t = false,
            help = "Whether the collection is rebaseable"
        )]
        rebaseable: bool,
        #[arg(
            long = "fee-rate",
            value_name = "SAT/VB",
            required = false,
            help = "Fee rate in sat/vB (optional)"
        )]
        fee_rate: Option<f64>,
        #[arg(
            long,
            value_name = "PASSPHRASE",
            help = "Passphrase for signing",
            required = false
        )]
        passphrase: Option<String>,
    },
    #[command(
        about = "Send a specific amount to an address",
        long_about = "Build and broadcast a transaction that sends the specified amount to the provided target address. Optionally set a custom fee rate (sat/vB)."
    )]
    SendAmount {
        #[arg(value_name = "ADDRESS", help = "Target address to receive the funds")]
        to: String,
        #[arg(
            long = "amount-sat",
            value_name = "SATOSHI",
            required = true,
            help = "Amount to send in satoshi"
        )]
        amount_sat: u64,
        #[arg(
            long = "fee-rate",
            value_name = "SAT/VB",
            required = false,
            help = "Fee rate in sat/vB (optional)"
        )]
        fee_rate: Option<f64>,
        #[arg(
            long,
            value_name = "PASSPHRASE",
            help = "Passphrase for signing",
            required = false
        )]
        passphrase: Option<String>,
    },
    #[command(
        about = "Send a raw OP_RETURN output",
        long_about = "Build and broadcast a transaction containing a custom OP_RETURN output with arbitrary hex payload. Optionally set a custom fee rate (sat/vB)."
    )]
    RawOutput {
        #[arg(
            long = "hex",
            value_name = "HEX",
            help = "Hex-encoded payload for OP_RETURN",
            required = true
        )]
        hex: String,
        #[arg(
            long = "fee-rate",
            value_name = "SAT/VB",
            required = false,
            help = "Fee rate in sat/vB (optional)"
        )]
        fee_rate: Option<f64>,
        #[arg(
            long,
            value_name = "PASSPHRASE",
            help = "Passphrase for signing",
            required = false
        )]
        passphrase: Option<String>,
    },

}

pub fn parse() -> Cli {
    let dotenv_path = env::var("DOTENV_PATH").unwrap_or(".env".into());
    dotenvy::from_filename(&dotenv_path).ok();

    println!("Loaded env from {}", dotenv_path);
    Cli::parse()
}
