use super::CommandRunner;
use crate::wallet::brc721_wallet::Brc721Wallet;
use crate::{cli, context};
use anyhow::{Context, Result};
use url::Url;

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        let net = ctx.network;

        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
                rescan,
            } => {
                // let m = Mnemonic::parse_in(Language::English, mnemonic.unwrap()).expect("mnemonic");
                // let wallet = Brc721Wallet::create(ctx.data_dir, ctx.network, m);
                //
                // log::info!("watch-only wallet '{}' ready in Core", wo_name);
                Ok(())
            }
            cli::WalletCmd::Address => {
                let mut wallet = Brc721Wallet::load(&ctx.rpc_url, ctx.network)
                    .context("loading wallet")?
                    .ok_or_else(|| anyhow::anyhow!("wallet not found"))?;

                let addr = wallet
                    .reveal_next_payment_address()
                    .context("getting address")?;

                log::info!("{}", addr);
                Ok(())
            }
            cli::WalletCmd::Balance => {
                let wallet = Brc721Wallet::load(&ctx.rpc_url, ctx.network)
                    .context("loading wallet")?
                    .ok_or_else(|| anyhow::anyhow!("wallet not found"))?;

                let url = Url::parse(&ctx.rpc_url)?;
                let balances = wallet.balance(&url, ctx.auth.clone())?;
                log::info!("{:?}", balances);
                Ok(())
            }
        }
    }
}
