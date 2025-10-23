use super::CommandRunner;
use crate::cli;
use crate::network;
use crate::wallet::Wallet;
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
                rescan,
            } => {
                let w = Wallet::new(&cli.data_dir, net);
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

                // compute default watch-only wallet name if not provided
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
                        format!("brc721-{}-{}", short, cli.network)
                    }
                };

                w.setup_watchonly(
                    &cli.rpc_url,
                    &cli.rpc_user,
                    &cli.rpc_pass,
                    &wo_name,
                    *rescan,
                )
                .context("setting up Core watch-only wallet")?;

                log::info!("watch-only wallet '{}' ready in Core", wo_name);
                Ok(())
            }
            cli::WalletCmd::Address { peek: _, change } => {
                let keychain = if *change {
                    KeychainKind::Internal
                } else {
                    KeychainKind::External
                };
                let w = Wallet::new(&cli.data_dir, net);
                let addr = w.address(keychain).context("getting address")?;
                log::info!("{addr}");
                Ok(())
            }
            cli::WalletCmd::List { all } => {
                use bitcoincore_rpc::RpcApi;
                let auth = match (&cli.rpc_user, &cli.rpc_pass) {
                    (Some(user), Some(pass)) => {
                        bitcoincore_rpc::Auth::UserPass(user.clone(), pass.clone())
                    }
                    _ => bitcoincore_rpc::Auth::None,
                };
                let base_url = cli.rpc_url.trim_end_matches('/').to_string();

                let local_path = crate::wallet::paths::wallet_db_path(&cli.data_dir, net);
                let local_exists = std::fs::metadata(&local_path).is_ok();
                if local_exists {
                    println!("Local:");
                    println!("  network={} path={}", cli.network, local_path.display());
                }

                let root = bitcoincore_rpc::Client::new(&base_url, auth.clone())
                    .context("creating root RPC client")?;
                let loaded: Vec<String> = root.list_wallets()?;
                println!("Core (loaded):");
                for name in &loaded {
                    let wallet_url = format!("{}/wallet/{}", base_url, name);
                    let wcli = bitcoincore_rpc::Client::new(&wallet_url, auth.clone())
                        .context("creating wallet RPC client")?;
                    let info: serde_json::Value = wcli.call("getwalletinfo", &[])?;
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

                if *all {
                    println!("Core (on-disk):");
                    let dir: serde_json::Value = root.call("listwalletdir", &[])?;
                    if let Some(arr) = dir.get("wallets").and_then(|v| v.as_array()) {
                        for w in arr {
                            if let Some(name) = w.get("name").and_then(|v| v.as_str()) {
                                if !loaded.iter().any(|lw| lw == name) {
                                    println!("  name={} unloaded", name);
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            cli::WalletCmd::Balance => {
                let w = Wallet::new(&cli.data_dir, net);
                let wallet_name = "brc721-watchonly";
                let bal = w
                    .core_balance(&cli.rpc_url, &cli.rpc_user, &cli.rpc_pass, wallet_name)
                    .context("reading core balance")?;
                log::info!("{bal}");
                Ok(())
            }
        }
    }
}
