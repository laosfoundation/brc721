use super::CommandRunner;
use crate::cli;
use crate::network;
use crate::wallet::{
    derive_next_address,
    get_core_balance,
    init_wallet,
    peek_address,
    setup_watchonly,
};
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
                gap,
                rescan,
            } => {
                let res = init_wallet(&cli.data_dir, net, mnemonic.clone(), passphrase.clone())
                    .context("Initializing wallet")?;
                if res.created {
                    log::info!("initialized wallet db={}", res.db_path.display());
                    if let Some(m) = res.mnemonic {
                        println!("{}", m);
                    }
                } else {
                    log::info!("wallet already initialized db={}", res.db_path.display());
                }

                setup_watchonly(
                    &cli.data_dir,
                    net,
                    &cli.rpc_url,
                    &cli.rpc_user,
                    &cli.rpc_pass,
                    watchonly,
                    *gap,
                    *rescan,
                )
                .context("setting up Core watch-only wallet")?;

                log::info!("watch-only wallet '{}' ready in Core", watchonly);
                Ok(())
            }
            cli::WalletCmd::Address { peek, change } => {
                let keychain = if *change {
                    KeychainKind::Internal
                } else {
                    KeychainKind::External
                };
                let addr = if let Some(index) = peek {
                    peek_address(&cli.data_dir, net, keychain, *index)
                        .context("peeking address")?
                } else {
                    derive_next_address(&cli.data_dir, net, keychain)
                        .context("deriving next address")?
                };
                log::info!("{addr}");
                Ok(())
            }
            cli::WalletCmd::Balance => {
                let base_url = cli.rpc_url.trim_end_matches('/').to_string();
                let wallet_name = "brc721-watchonly";
                let bal = get_core_balance(&base_url, &cli.rpc_user, &cli.rpc_pass, wallet_name)
                    .context("reading core balance")?;
                log::info!("{bal}");
                Ok(())
            }
        }
    }
}
