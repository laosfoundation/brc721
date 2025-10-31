use super::CommandRunner;
use crate::wallet::brc721_wallet::Brc721Wallet;
use crate::{cli, context};
use anyhow::{Context, Result};
use bdk_wallet::bip39::{Language, Mnemonic};

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        match self {
            cli::WalletCmd::Init { mnemonic, passphrase } => {
                // get or generate mnemonic
                let mnemonic = mnemonic
                    .as_ref()
                    .map(|m| Mnemonic::parse_in(Language::English, m).expect("invalid mnemonic"));

                log::info!("👛 Loading or creating wallet...");
                let wallet = Brc721Wallet::load_or_create(&ctx.data_dir, ctx.network, mnemonic, passphrase.clone())
                    .context("creating wallet")?;

                wallet
                    .setup_watch_only(&ctx.rpc_url, ctx.auth.clone())
                    .expect("setup watch only");

                log::info!("📡 Watch-only wallet '{}' ready in Core", wallet.id());
                Ok(())
            }
            cli::WalletCmd::Address => {
                let mut wallet = Brc721Wallet::load(&ctx.data_dir, ctx.network)
                    .context("loading wallet")?
                    .ok_or_else(|| anyhow::anyhow!("wallet not found"))?;

                let addr = wallet
                    .reveal_next_payment_address()
                    .context("getting address")?;

                log::info!("🏠 {}", addr);
                Ok(())
            }
            cli::WalletCmd::Balance => {
                let wallet = Brc721Wallet::load(&ctx.data_dir, ctx.network)
                    .context("loading wallet")?
                    .ok_or_else(|| anyhow::anyhow!("wallet not found"))?;

                let balances = wallet.balance(&ctx.rpc_url, ctx.auth.clone())?;
                log::info!("💰 {:?}", balances);
                Ok(())
            }
        }
    }
}
