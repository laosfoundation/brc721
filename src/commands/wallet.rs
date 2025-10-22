use super::CommandRunner;
use crate::cli;
use crate::network;
use crate::wallet::{derive_next_address, init_wallet, peek_address};
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
                gap,
                rescan,
            } => {
                let res = init_wallet(&cli.data_dir, net, mnemonic.clone(), passphrase.clone())
                    .context("Initializing wallet")?;
                if res.created {
                    log::info!("initialized wallet db={}", res.db_path.display());
                    if let Some(m) = res.mnemonic {
                        println!("{}", m);
                    }
                } else {
                    log::info!("wallet already initialized db={}", res.db_path.display());
                }

                // Also set up (or refresh) the Core watch-only wallet with our public descriptors
                use bitcoincore_rpc::RpcApi;
                use serde_json::json;

                let auth = match (&cli.rpc_user, &cli.rpc_pass) {
                    (Some(user), Some(pass)) =>
                        bitcoincore_rpc::Auth::UserPass(user.clone(), pass.clone()),
                    _ => bitcoincore_rpc::Auth::None,
                };
                let base_url = cli.rpc_url.trim_end_matches('/').to_string();
                let root = bitcoincore_rpc::Client::new(&base_url, auth.clone())
                    .context("creating root RPC client")?;

                let db_path = crate::wallet::paths::wallet_db_path(&cli.data_dir, net);
                let mut conn = rusqlite::Connection::open(&db_path)
                    .with_context(|| format!("opening wallet db at {}", db_path.display()))?;

                let wallet = bdk_wallet::LoadParams::new()
                    .check_network(net)
                    .load_wallet(&mut conn)?
                    .ok_or_else(|| anyhow::anyhow!("wallet not initialized"))?;

                let ext_desc = wallet
                    .public_descriptor(bdk_wallet::KeychainKind::External)
                    .to_string();
                let int_desc = wallet
                    .public_descriptor(bdk_wallet::KeychainKind::Internal)
                    .to_string();
                let ext_cs = wallet.descriptor_checksum(bdk_wallet::KeychainKind::External);
                let int_cs = wallet.descriptor_checksum(bdk_wallet::KeychainKind::Internal);
                let ext_with_cs = format!("{}#{}", ext_desc, ext_cs);
                let int_with_cs = format!("{}#{}", int_desc, int_cs);

                // Best-effort create watch-only wallet (ignore if exists)
                let _ = root.call::<serde_json::Value>(
                    "createwallet",
                    &[
                        json!(watchonly),
                        json!(true),  // disable_private_keys
                        json!(true),  // blank
                        json!(""),    // passphrase
                        json!(false), // avoid_reuse
                        json!(true),  // descriptors
                    ],
                );

                let wallet_url = format!("{}/wallet/{}", base_url, watchonly);
                let wallet_rpc = bitcoincore_rpc::Client::new(&wallet_url, auth)
                    .context("creating wallet RPC client")?;

                let end = (*gap as u32).saturating_sub(1);
                let ts_val = if *rescan { json!(0) } else { json!("now") };

                let imports = json!([
                    {
                        "desc": ext_with_cs,
                        "active": true,
                        "range": [0, end],
                        "timestamp": ts_val,
                        "internal": false,
                        "label": "brc721-external"
                    },
                    {
                        "desc": int_with_cs,
                        "active": true,
                        "range": [0, end],
                        "timestamp": ts_val,
                        "internal": true,
                        "label": "brc721-internal"
                    }
                ]);

                let _res: serde_json::Value = wallet_rpc
                    .call("importdescriptors", &[imports])
                    .context("importing public descriptors to Core")?;

                log::info!("watch-only wallet '{}' ready in Core", watchonly);
                Ok(())
            }
            cli::WalletCmd::Address { peek, change } => {
                let keychain = if *change {
                    KeychainKind::Internal
                } else {
                    KeychainKind::External
                };
                let addr = if let Some(index) = peek {
                    peek_address(&cli.data_dir, net, keychain, *index)
                        .context("peeking address")?
                } else {
                    derive_next_address(&cli.data_dir, net, keychain)
                        .context("deriving next address")?
                };
                log::info!("{addr}");
                Ok(())
            }
            cli::WalletCmd::Balance => {
                // Read balance from Core watch-only (default name)
                use bitcoincore_rpc::RpcApi;
                let auth = match (&cli.rpc_user, &cli.rpc_pass) {
                    (Some(user), Some(pass)) =>
                        bitcoincore_rpc::Auth::UserPass(user.clone(), pass.clone()),
                    _ => bitcoincore_rpc::Auth::None,
                };
                let base_url = cli.rpc_url.trim_end_matches('/').to_string();
                let wallet_name = "brc721-watchonly";
                let wallet_url = format!("{}/wallet/{}", base_url, wallet_name);
                let rpc = bitcoincore_rpc::Client::new(&wallet_url, auth)
                    .context("creating wallet RPC client")?;
                let bal = rpc.get_balance(None, None)?;
                log::info!("{bal}");
                Ok(())
            }
        }
    }
}
