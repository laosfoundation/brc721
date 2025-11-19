use super::CommandRunner;
use crate::wallet::brc721_wallet::Brc721Wallet;
use crate::wallet::passphrase::prompt_passphrase;
use crate::{cli, context};
use age::secrecy::SecretString;
use anyhow::{Context, Result};
use bdk_wallet::bip39::{Language, Mnemonic};

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        let loaded = Brc721Wallet::load(&ctx.data_dir, ctx.network, &ctx.rpc_url, ctx.auth.clone());

        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
            } => {
                // get or generate mnemonic
                let mnemonic = mnemonic
                    .as_ref()
                    .map(|m| Mnemonic::parse_in(Language::English, m).expect("invalid mnemonic"));

                let wallet = loaded
                    .or_else(|_| {
                        let passphrase =
                            passphrase
                                .clone()
                                .map(SecretString::from)
                                .unwrap_or_else(|| {
                                    SecretString::from(
                                        prompt_passphrase().expect("prompt").unwrap_or_default(),
                                    )
                                });
                        let w = Brc721Wallet::create(
                            &ctx.data_dir,
                            ctx.network,
                            mnemonic,
                            passphrase,
                            &ctx.rpc_url,
                            ctx.auth.clone(),
                        );
                        log::info!("🎉 New wallet created");
                        w
                    })
                    .context("wallet initialization")?;

                wallet.setup_watch_only().expect("setup watch only");

                log::info!("📡 Watch-only wallet '{}' ready in Core", wallet.id());
                Ok(())
            }
            cli::WalletCmd::Address => {
                let mut wallet = loaded.context("loading wallet")?;
                let addr = wallet
                    .reveal_next_payment_address()
                    .context("getting address")?;
                log::info!("🏠 {}", addr.address);
                Ok(())
            }
            cli::WalletCmd::Balance => {
                let wallet = loaded.context("loading wallet")?;
                let balances = wallet.balances()?;
                log::info!("💰 {:?}", balances);
                Ok(())
            }
            cli::WalletCmd::Rescan => {
                let wallet = loaded.context("loading wallet")?;
                wallet
                    .rescan_watch_only()
                    .context("rescan watch-only wallet")?;
                log::info!("🔄 Rescan started for watch-only wallet '{}'", wallet.id());
                Ok(())
            }
        }
    }
}
