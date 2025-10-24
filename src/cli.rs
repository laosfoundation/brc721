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
            help = "Optional BIP39 passphrase",
            required = false
        )]
        passphrase: Option<String>,
        #[arg(
            long = "rescan",
            default_value_t = false,
            help = "Full rescan from genesis for imported descriptors"
        )]
        rescan: bool,
    },
    #[command(
        about = "Get a receive address",
        long_about = "Derive and display the next receive address."
    )]
    Address,
    #[command(
        about = "Show wallet balance",
        long_about = "Display confirmed and unconfirmed wallet balances as tracked via the Core watch-only wallet and local index."
    )]
    Balance,
    #[command(
        about = "List known wallets",
        long_about = "List discovered or configured wallets loaded in Bitcoin Core and the local database."
    )]
    List,
    #[command(
        about = "Show extended public keys",
        long_about = "Display the external and internal BIP86 extended public keys (xpub-like) for the wallet."
    )]
    Xpub,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TxCmd {
    #[command(
        about = "Register a BRC-721 collection",
        long_about = "Create and broadcast a transaction that registers a BRC-721 collection, linking a 20-byte EVM (H160) address. Optionally mark the collection as rebaseable and set a custom fee rate (sat/vB)."
    )]
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
