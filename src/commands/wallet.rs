use super::CommandRunner;
use crate::wallet::Wallet;
use crate::{cli, context};
use anyhow::{Context, Result};
use bdk_wallet::KeychainKind;
use std::str::FromStr;
use bitcoin::address::NetworkUnchecked;

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

                let wo_name = w.generate_wallet_name()?;

                w.setup_watchonly(&ctx.rpc_url, &ctx.auth, &wo_name, *rescan)
                    .context("setting up Core watch-only wallet")?;

                log::info!("watch-only wallet '{}' ready in Core", wo_name);
                Ok(())
            }
            cli::WalletCmd::Send { to, amount, fee_rate, esplora } => {
                let addr_un = bitcoin::Address::<NetworkUnchecked>::from_str(to)
                    .map_err(|e| anyhow::anyhow!(e))?;
                let addr = addr_un
                    .require_network(ctx.network)
                    .map_err(|e| anyhow::anyhow!(e))?;
                let txid = w
                    .send(esplora, &addr, bitcoin::Amount::from_sat(*amount), *fee_rate)
                    .context("wallet send")?;
                log::info!("{txid}");
                Ok(())
            }
            cli::WalletCmd::Address => {
                let keychain = KeychainKind::External;
                let addr = w.address(keychain).context("getting address")?;
                log::info!("{addr}");
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
                        info.name, info.watch_only, info.descriptors
                    );
                }

                Ok(())
            }
            cli::WalletCmd::Balance => {
                let wallet_name = "brc721-watchonly";
                let bal = w
                    .core_balance(&ctx.rpc_url, &ctx.auth, wallet_name)
                    .context("reading core balance")?;
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
