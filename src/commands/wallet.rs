use super::CommandRunner;
use crate::cli;
use crate::network;
use crate::wallet::Wallet;
use anyhow::{Context, Result};
use bdk_wallet::KeychainKind;

impl CommandRunner for cli::WalletCmd {
    fn run(&self, cli: &cli::Cli) -> Result<()> {
        let net = network::parse_network(Some(cli.network.clone()));
        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
                watchonly,
                rescan,
            } => {
                let w = Wallet::new(&cli.data_dir, net);
                let res = w
                    .init(mnemonic.clone(), passphrase.clone())
                    .context("Initializing wallet")?;
                if res.created {
                    log::info!("initialized wallet db={}", res.db_path.display());
                    if let Some(m) = res.mnemonic {
                        println!("{}", m);
                    }
                } else {
                    log::info!("wallet already initialized db={}", res.db_path.display());
                }

                w.setup_watchonly(
                    &cli.rpc_url,
                    &cli.rpc_user,
                    &cli.rpc_pass,
                    watchonly,
                    *rescan,
                )
                .context("setting up Core watch-only wallet")?;

                log::info!("watch-only wallet '{}' ready in Core", watchonly);
                Ok(())
            }
            cli::WalletCmd::Address { peek: _, change } => {
                let keychain = if *change {
                    KeychainKind::Internal
                } else {
                    KeychainKind::External
                };
                let w = Wallet::new(&cli.data_dir, net);
                let addr = w.address(keychain).context("getting address")?;
                log::info!("{addr}");
                Ok(())
            }
            cli::WalletCmd::Balance => {
                let w = Wallet::new(&cli.data_dir, net);
                let wallet_name = "brc721-watchonly";
                let bal = w
                    .core_balance(&cli.rpc_url, &cli.rpc_user, &cli.rpc_pass, wallet_name)
                    .context("reading core balance")?;
                log::info!("{bal}");
                Ok(())
            }
        }
    }
}
