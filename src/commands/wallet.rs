use super::CommandRunner;
use crate::{cli, context};
use crate::wallet::{derive_next_address, init_wallet, peek_address};
use anyhow::{Context, Result};
use bdk_wallet::KeychainKind;

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        let net = ctx.network;
        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
            } => {
                let res = init_wallet(&ctx.data_dir, net, mnemonic.clone(), passphrase.clone())
                    .context("Initializing wallet")?;
                if res.created {
                    log::info!("initialized wallet db={}", res.db_path.display());
                    if let Some(m) = res.mnemonic {
                        println!("{}", m);
                    }
                } else {
                    log::info!("wallet already initialized db={}", res.db_path.display());
                }

                Ok(())
            }
            cli::WalletCmd::Address { peek, change } => {
                let keychain = if *change {
                    KeychainKind::Internal
                } else {
                    KeychainKind::External
                };
                let addr = if let Some(index) = peek {
                    peek_address(&ctx.data_dir, net, keychain, *index)
                        .context("peeking address")?
                } else {
                    derive_next_address(&ctx.data_dir, net, keychain)
                        .context("deriving next address")?
                };
                log::info!("{addr}");
                Ok(())
            }
        }
    }
}
