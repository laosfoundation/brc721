use super::CommandRunner;
use crate::wallet::brc721_wallet::Brc721Wallet;
use crate::wallet::Wallet;
use crate::{cli, context};
use anyhow::{Context, Result};
use bdk_wallet::bip39::{Language, Mnemonic};
use bdk_wallet::KeychainKind;
use url::Url;

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        let net = ctx.network;
        let w = Wallet::new(&ctx.data_dir, net);

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
            cli::WalletCmd::List => {
                let local_path = w.local_db_path();
                if std::fs::metadata(&local_path).is_ok() {
                    log::info!("Local:");
                    log::info!("  network={} path={}", ctx.network, local_path.display());
                }

                let base_url = ctx.rpc_url.trim_end_matches('/').to_string();
                let rpc = crate::wallet::types::RealCoreRpc::new(base_url, ctx.auth.clone());
                let listed = w.list_core_wallets(&rpc)?;
                log::info!("Core (loaded):");
                for info in listed {
                    log::info!(
                        "  name={} watch_only={} descriptors={}",
                        info.name,
                        info.watch_only,
                        info.descriptors
                    );
                }

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
            cli::WalletCmd::Xpub => {
                let (ext, int) = w.public_descriptors_with_checksum()?;
                log::info!("external: {ext}");
                log::info!("internal: {int}");
                Ok(())
            }
        }
    }
}
