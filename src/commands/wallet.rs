use super::CommandRunner;
use crate::wallet::brc721_wallet::Brc721Wallet;
use crate::{cli, context};
use anyhow::{Context, Result};
use bdk_wallet::bip39::{Language, Mnemonic};

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
            } => {
                // get or generate mnemonic
                let mnemonic = mnemonic
                    .as_ref()
                    .map(|m| Mnemonic::parse_in(Language::English, m).expect("invalid mnemonic"));

                let wallet = Brc721Wallet::load(&ctx.data_dir, ctx.network)
                    .or_else(|_| {
                        let w = Brc721Wallet::create(
                            &ctx.data_dir,
                            ctx.network,
                            mnemonic,
                            passphrase.clone(),
                        );
                        log::info!("🎉 New wallet created");
                        w
                    })
                    .context("wallet initialization")?;

                wallet
                    .setup_watch_only(&ctx.rpc_url, ctx.auth.clone())
                    .expect("setup watch only");

                log::info!("📡 Watch-only wallet '{}' ready in Core", wallet.id());
                Ok(())
            }
            cli::WalletCmd::Address => {
                let mut wallet =
                    Brc721Wallet::load(&ctx.data_dir, ctx.network).context("loading wallet")?;

                let addr = wallet
                    .reveal_next_payment_address()
                    .context("getting address")?;

                log::info!("🏠 {}", addr.address);
                Ok(())
            }
            cli::WalletCmd::Balance => {
                let wallet =
                    Brc721Wallet::load(&ctx.data_dir, ctx.network).context("loading wallet")?;

                let balances = wallet.balances(&ctx.rpc_url, ctx.auth.clone())?;
                log::info!("💰 {:?}", balances);
                Ok(())
            }
            cli::WalletCmd::Rescan => {
                let wallet =
                    Brc721Wallet::load(&ctx.data_dir, ctx.network).context("loading wallet")?;

                wallet
                    .rescan_watch_only(&ctx.rpc_url, ctx.auth.clone())
                    .context("rescan watch-only wallet")?;
                log::info!("🔄 Rescan started for watch-only wallet '{}'", wallet.id());
                Ok(())
            }
        }
    }
}
