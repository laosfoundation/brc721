use std::str::FromStr;

use super::CommandRunner;
use crate::{cli, context, wallet::brc721_wallet::Brc721Wallet};
use anyhow::{Context, Result};
use bitcoin::{Address, Amount};

impl CommandRunner for cli::TxCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        match self {
            cli::TxCmd::RegisterCollection {
                laos_hex,
                rebaseable,
                fee_rate,
                passphrase,
            } => {
                use crate::types::{build_register_collection_tx, RegisterCollectionMessage};
                use bitcoin::psbt::Psbt;
                use bitcoincore_rpc::{Client, RpcApi};
                use ethereum_types::H160;

                let wallet = Brc721Wallet::load(&ctx.data_dir, ctx.network)?;

                // Parse 20-byte hex EVM address
                let laos = H160::from_slice(&hex::decode(laos_hex)?);
                let msg = RegisterCollectionMessage {
                    collection_address: laos,
                    rebaseable: *rebaseable,
                };
                let brc_tx = build_register_collection_tx(&msg);
                let outputs = brc_tx.output;
                let mut psbt = wallet
                    .create_psbt_from_txouts(&ctx.rpc_url, ctx.auth.clone(), outputs, *fee_rate)
                    .unwrap();

                let finalized = wallet
                    .sign(&mut psbt, passphrase.clone())
                    .context("psbt sign")?;
                assert!(finalized);
                let tx = psbt
                    .extract_tx()
                    .map_err(|e| anyhow::anyhow!("extract_tx: {e}"))?;
                // Broadcast using root client since wallet client may lack mempool policy overrides
                let txid = Client::new(ctx.rpc_url.as_ref(), ctx.auth.clone())?
                    .send_raw_transaction(&tx)?;

                log::info!(
                    "✅ Registered collection {}, rebaseable: {}, txid: {}",
                    laos_hex,
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
                let wallet = Brc721Wallet::load(&ctx.data_dir, ctx.network)?;
                let amount = Amount::from_sat(*amount_sat);
                let address = Address::from_str(to)?.require_network(ctx.network)?;
                wallet.send_amount(
                    &ctx.rpc_url,
                    ctx.auth.clone(),
                    &address,
                    amount,
                    *fee_rate,
                    passphrase.clone(),
                )?;
                log::info!("✅ Sent {} sat to {}", amount_sat, to);
                Ok(())
            }
        }
    }
}
