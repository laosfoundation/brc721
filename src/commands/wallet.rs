use super::CommandRunner;
use crate::wallet::Wallet;
use crate::{cli, context};
use anyhow::{Context, Result};
use bdk_wallet::KeychainKind;

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        let net = ctx.network;
        let w = Wallet::new(&ctx.data_dir, net);

        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
            } => {
                let res = w
                    .init(mnemonic.clone(), passphrase.clone())
                    .context("Initializing wallet")?;
                if res.created {
                    log::info!("initialized wallet db={}", res.db_path.display());
                    if let Some(m) = res.mnemonic {
                        log::info!("{}", m);
                    }
                } else {
                    log::info!("wallet already initialized db={}", res.db_path.display());
                }

                Ok(())
            }
            cli::WalletCmd::Address => {
                let keychain = KeychainKind::External;
                let addr = w.address(keychain).context("getting address")?;
                log::info!("{addr}");
                Ok(())
            }
            cli::WalletCmd::Balance => {
                let bal = w.balance().context("reading core balance")?;
                log::info!("{bal}");
                Ok(())
            }
            cli::WalletCmd::Xpub => {
                let (ext, int) = w.public_descriptors_with_checksum()?;
                log::info!("external: {ext}");
                log::info!("internal: {int}");
                Ok(())
            }
        }
    }
}
