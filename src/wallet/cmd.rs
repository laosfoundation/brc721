use bitcoincore_rpc::{Auth, Client};

use crate::cli;
use crate::wallet::{init_wallet, next_address};
use bitcoin as _; // ensure bitcoin crate in scope for network type

pub fn handle_wallet_command(cli: &cli::Cli, net: bitcoin::network::Network, wcmd: cli::WalletCmd) {
    match wcmd {
        cli::WalletCmd::Init { mnemonic, passphrase } => {
            let _ = std::fs::create_dir_all(&cli.data_dir);
            match init_wallet(&cli.data_dir, net, mnemonic, passphrase) {
                Ok(res) => {
                    if res.created {
                        if let Some(m) = res.mnemonic {
                            log::info!(
                                "initialized wallet db={} mnemonic=\"{}\"",
                                res.db_path.display(),
                                m
                            );
                        } else {
                            log::info!("initialized wallet db={}", res.db_path.display());
                        }
                    } else {
                        log::info!("wallet already initialized db={}", res.db_path.display());
                    }
                }
                Err(e) => {
                    log::error!("wallet init error: {}", e);
                }
            }
        }
        cli::WalletCmd::Address => {
            let _ = std::fs::create_dir_all(&cli.data_dir);
            match next_address(&cli.data_dir, net) {
                Ok(addr) => {
                    log::info!("{}", addr);
                }
                Err(e) => {
                    log::error!("wallet address error: {}", e);
                }
            }
        }
        cli::WalletCmd::RegisterCollection {
            laos_hex,
            rebaseable,
            fee_rate,
        } => {
            let auth = match (&cli.rpc_user, &cli.rpc_pass) {
                (Some(user), Some(pass)) => Auth::UserPass(user.clone(), pass.clone()),
                _ => Auth::None,
            };
            let client = Client::new(&cli.rpc_url, auth).expect("failed to create RPC client");

            match crate::wallet::tx::send_register_collection(&client, &laos_hex, rebaseable, fee_rate) {
                Ok(txid) => {
                    log::info!("broadcasted register-collection txid={}", txid);
                }
                Err(e) => {
                    log::error!("failed to send register-collection: {}", e);
                }
            }
        }
    }
}
