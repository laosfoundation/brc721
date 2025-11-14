use std::str::FromStr;

use super::CommandRunner;
use crate::types::{brc721_output, RegisterCollectionMessage};
use crate::wallet::passphrase::prompt_passphrase_once;
use crate::{cli, context, wallet::brc721_wallet::Brc721Wallet};
use anyhow::{Context, Result};
use bitcoin::{Address, Amount};

impl CommandRunner for cli::TxCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        match self {
            cli::TxCmd::RegisterCollection {
                collection_address,
                rebaseable,
                fee_rate,
                passphrase,
            } => {
                let msg = RegisterCollectionMessage {
                    collection_address: *collection_address,
                    rebaseable: *rebaseable,
                };
                let output = brc721_output(&msg.encode());

                let wallet =
                    Brc721Wallet::load(&ctx.data_dir, ctx.network, &ctx.rpc_url, ctx.auth.clone())?;
                let passphrase = passphrase.clone().unwrap_or_else(|| {
                    prompt_passphrase_once()
                        .expect("prompt")
                        .unwrap_or_default()
                });
                let tx = wallet
                    .build_tx(vec![output], *fee_rate, passphrase)
                    .context("build tx")?;
                let txid = wallet.broadcast(&tx)?;

                log::info!(
                    "✅ Registered collection {:#x}, rebaseable: {}, txid: {}",
                    collection_address,
                    rebaseable,
                    txid
                );
                Ok(())
            }
            cli::TxCmd::SendAmount {
                to,
                amount_sat,
                fee_rate,
                passphrase,
            } => {
                let wallet =
                    Brc721Wallet::load(&ctx.data_dir, ctx.network, &ctx.rpc_url, ctx.auth.clone())?;
                let amount = Amount::from_sat(*amount_sat);
                let address = Address::from_str(to)?.require_network(ctx.network)?;
                let passphrase = passphrase.clone().unwrap_or_else(|| {
                    prompt_passphrase_once()
                        .expect("prompt")
                        .unwrap_or_default()
                });
                let tx = wallet
                    .build_payment_tx(&address, amount, *fee_rate, passphrase)
                    .context("build payment tx")?;
                let txid = wallet.broadcast(&tx)?;
                log::info!("✅ Sent {} sat to {} (txid: {})", amount_sat, to, txid);
                Ok(())
            }
        }
    }
}
