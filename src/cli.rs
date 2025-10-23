use clap::{Parser, Subcommand};

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
        long,
        value_name = "NETWORK",
        help = "bitcoin|testnet|signet|regtest",
        default_value = "bitcoin"
    )]
    pub network: String,

    #[command(subcommand)]
    pub cmd: Option<Command>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    Wallet {
        #[command(subcommand)]
        cmd: WalletCmd,
    },
    Tx {
        #[command(subcommand)]
        cmd: TxCmd,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum WalletCmd {
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
            help = "Optional BIP39 passphrase",
            required = false
        )]
        passphrase: Option<String>,
        #[arg(
            long = "watchonly",
            value_name = "NAME",
            help = "Core watch-only wallet name (optional, defaults to a unique name)",
            required = false
        )]
        watchonly: Option<String>,

        #[arg(
            long = "rescan",
            default_value_t = false,
            help = "Full rescan from genesis for imported descriptors"
        )]
        rescan: bool,
    },
    Address {
        #[arg(
            long,
            value_name = "INDEX",
            help = "Peek address at INDEX without advancing",
            required = false
        )]
        peek: Option<u32>,
        #[arg(long, default_value_t = false, help = "Use change (internal) keychain")]
        change: bool,
    },
    Balance,
    List {
        #[arg(
            long = "all",
            default_value_t = false,
            help = "Include unloaded wallets found on disk (admin only)"
        )]
        all: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum TxCmd {
    RegisterCollection {
        #[arg(
            long = "laos-hex",
            value_name = "20-BYTE-HEX",
            help = "20-byte hex collection address (EVM H160)",
            required = true
        )]
        laos_hex: String,
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
    },
}

pub fn parse() -> Cli {
    let _ = dotenvy::dotenv();
    Cli::parse()
}
