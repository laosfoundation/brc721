use super::CommandRunner;
use crate::wallet::types::CoreRpc;
use crate::wallet::Wallet;
use crate::{cli, context};
use anyhow::{Context, Result};
use bdk_wallet::KeychainKind;

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        let net = ctx.network;
        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
                watchonly,
                rescan,
            } => {
                let w = Wallet::new(&ctx.data_dir, net);
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

                let wo_name = match watchonly.clone() {
                    Some(name) => name,
                    None => {
                        let (ext_with_cs, _int_with_cs) = w
                            .public_descriptors_with_checksum()
                            .context("loading public descriptors")?;
                        let mut hasher = sha2::Sha256::new();
                        use sha2::Digest;
                        hasher.update(ext_with_cs.as_bytes());
                        let hash = hasher.finalize();
                        let short = hex::encode(&hash[..4]);
                        format!("brc721-{}-{}", short, ctx.network)
                    }
                };

                let (rpc_user, rpc_pass) = match &ctx.auth {
                    bitcoincore_rpc::Auth::UserPass(user, pass) => {
                        (Some(user.clone()), Some(pass.clone()))
                    }
                    _ => (None, None),
                };
                w.setup_watchonly(&ctx.rpc_url, &rpc_user, &rpc_pass, &wo_name, *rescan)
                    .context("setting up Core watch-only wallet")?;

                log::info!("watch-only wallet '{}' ready in Core", wo_name);
                Ok(())
            }
            cli::WalletCmd::Address => {
                let keychain = KeychainKind::External;
                let w = Wallet::new(&ctx.data_dir, net);
                let addr = w.address(keychain).context("getting address")?;
                log::info!("{addr}");
                Ok(())
            }
            cli::WalletCmd::List => {
                let base_url = ctx.rpc_url.trim_end_matches('/').to_string();

                let local_path = crate::wallet::paths::wallet_db_path(&ctx.data_dir, net);
                let local_exists = std::fs::metadata(&local_path).is_ok();
                if local_exists {
                    println!("Local:");
                    println!("  network={} path={}", ctx.network, local_path.display());
                }

                let rpc =
                    crate::wallet::types::RealCoreRpc::new(base_url.clone(), ctx.auth.clone());
                let loaded: Vec<String> = CoreRpc::list_wallets(&rpc)?;
                println!("Core (loaded):");
                for name in &loaded {
                    let info = CoreRpc::get_wallet_info(&rpc, name)?;
                    let pk_enabled = info
                        .get("private_keys_enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let descriptors = info
                        .get("descriptors")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let watch_only = !pk_enabled;
                    println!(
                        "  name={} watch_only={} descriptors={}",
                        name, watch_only, descriptors
                    );
                }

                Ok(())
            }
            cli::WalletCmd::Balance => {
                let w = Wallet::new(&ctx.data_dir, net);
                let wallet_name = "brc721-watchonly";
                let (user, pass) = match &ctx.auth {
                    bitcoincore_rpc::Auth::UserPass(user, pass) => {
                        (Some(user.clone()), Some(pass.clone()))
                    }
                    _ => (None, None),
                };
                let bal = w
                    .core_balance(&ctx.rpc_url, &user, &pass, wallet_name)
                    .context("reading core balance")?;
                log::info!("{bal}");
                Ok(())
            }
        }
    }
}
