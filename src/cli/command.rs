use clap::Subcommand;

use crate::cli::tx_cmd::TxCmd;
use crate::cli::wallet_cmd::WalletCmd;

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
        long_about = "Build and submit protocol transactions, such as registering BRC-721 collections and ownership."
    )]
    Tx {
        #[command(subcommand)]
        cmd: TxCmd,
    },
}
