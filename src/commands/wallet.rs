use super::CommandRunner;
use crate::cli;
use crate::wallet::{init_wallet, network, next_address};
use anyhow::{Context, Result};
use bitcoin::{self as _};
use bitcoincore_rpc::{Auth, Client}; // ensure bitcoin crate in scope for network type

impl CommandRunner for cli::WalletCmd {
    fn run(&self, cli: &cli::Cli) -> Result<()> {
        let net = network::parse_network(Some(cli.network.clone()));
        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
            } => {
                let res = init_wallet(&cli.data_dir, net, mnemonic.clone(), passphrase.clone())
                    .map_err(anyhow::Error::msg)
                    .context("Initializing wallet")?;
                if res.created {
                    if let Some(m) = res.mnemonic {
                        log::info!(
                            "initialized wallet db={} mnemonic=\"{}\"",
                            res.db_path.display(),
                            m
                        );
                    } else {
                        log::info!("initialized wallet db={}", res.db_path.display());
                    }
                } else {
                    log::info!("wallet already initialized db={}", res.db_path.display());
                }

                Ok(())
            }
            cli::WalletCmd::Address => {
                let addr = next_address(&cli.data_dir, net)
                    .map_err(anyhow::Error::msg)
                    .context("deriving next address")?;
                log::info!("{addr}");
                Ok(())
            }
        }
    }
}
