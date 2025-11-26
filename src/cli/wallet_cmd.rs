use clap::Subcommand;

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
        about = "Generate a new BIP39 mnemonic",
        long_about = "Generate a new BIP39 mnemonic phrase (24 words by default, 12 with --short)."
    )]
    Generate {
        #[arg(
            long = "short",
            help = "Generate a short 12-word mnemonic instead of 24-word",
            num_args(0),
            default_value_t = false
        )]
        short: bool,
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
