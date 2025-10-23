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

                w.setup_watchonly(
                    &cli.rpc_url,
                    &cli.rpc_user,
                    &cli.rpc_pass,
                    watchonly,
                    *rescan,
                )
                .context("setting up Core watch-only wallet")?;

                log::info!("watch-only wallet '{}' ready in Core", watchonly);
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
                    (Some(user), Some(pass)) => bitcoincore_rpc::Auth::UserPass(user.clone(), pass.clone()),
                    _ => bitcoincore_rpc::Auth::None,
                };
                let base_url = cli.rpc_url.trim_end_matches('/').to_string();

                let w = Wallet::new(&cli.data_dir, net);
                let local_path = crate::wallet::paths::wallet_db_path(&cli.data_dir, net);
                let local_exists = std::fs::metadata(&local_path).is_ok();
                if local_exists {
                    // presence checked further below while iterating loaded wallets via listdescriptors
                    let _ = w.public_descriptors_with_checksum();
                    println!("Local:");
                    println!("  network={} path={}", cli.network, local_path.display());
                }

                let root = bitcoincore_rpc::Client::new(&base_url, auth.clone())
                    .context("creating root RPC client")?;
                let loaded: Vec<String> = root.list_wallets()?;
                println!("Core (loaded):");
                // We will also determine if the local wallet descriptors are present in any loaded wallet
                let mut local_ext: Option<String> = None;
                let mut local_int: Option<String> = None;
                if local_exists {
                    if let Ok((e, i)) = w.public_descriptors_with_checksum() {
                        local_ext = Some(e);
                        local_int = Some(i);
                    }
                }
                let mut watched_any = false;
                let mut watchers: Vec<String> = Vec::new();
                for name in &loaded {
                    let wallet_url = format!("{}/wallet/{}", base_url, name);
                    let wcli = bitcoincore_rpc::Client::new(&wallet_url, auth.clone())
                        .context("creating wallet RPC client")?;
                    let info: serde_json::Value = wcli.call("getwalletinfo", &[])?;
                    let pk_enabled = info.get("private_keys_enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                    let descriptors = info.get("descriptors").and_then(|v| v.as_bool()).unwrap_or(false);
                    let watch_only = !pk_enabled;

                    // Try listdescriptors to see if this wallet watches our descriptors
                    let mut watches_local = false;
                    if descriptors {
                        if let Ok(descs) = wcli.call::<serde_json::Value>("listdescriptors", &[]) {
                            if let (Some(ref ext), Some(ref int)) = (&local_ext, &local_int) {
                                if let Some(arr) = descs.get("descriptors").and_then(|v| v.as_array()) {
                                    for d in arr {
                                        if let Some(desc_str) = d.get("desc").and_then(|v| v.as_str()) {
                                            if desc_str == ext || desc_str == int {
                                                watches_local = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if watches_local {
                        watched_any = true;
                        watchers.push(name.clone());
                    }

                    println!("  name={} watch_only={} descriptors={} watches_local={}",
                        name, watch_only, descriptors, watches_local);
                }
                if local_exists {
                    println!("Local watch status: watched_by_core={}{}",
                        watched_any,
                        if watched_any { format!(" ({})", watchers.join(", ")) } else { String::new() }
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
