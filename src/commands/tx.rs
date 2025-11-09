use std::str::FromStr;

use super::CommandRunner;
use crate::{cli, context, wallet::brc721_wallet::Brc721Wallet};
use anyhow::Result;
use bitcoin::{Address, Amount};
use bdk_wallet::miniscript::psbt::PsbtExt;

impl CommandRunner for cli::TxCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        match self {
            cli::TxCmd::RegisterCollection { laos_hex, rebaseable, fee_rate, passphrase } => {
                use crate::types::{build_register_collection_tx, RegisterCollectionMessage};
                use ethereum_types::H160;
                use bitcoin::psbt::Psbt;
                use bitcoincore_rpc::{Client, RpcApi};

                let wallet = Brc721Wallet::load(&ctx.data_dir, ctx.network)?;

                // Parse 20-byte hex EVM address
                let laos = H160::from_slice(&hex::decode(laos_hex)?);
                let msg = RegisterCollectionMessage { collection_address: laos, rebaseable: *rebaseable };
                let brc_tx = build_register_collection_tx(&msg);

                // Create a PSBT funding the OP_RETURN output
                let watch_name = wallet.id();
                let watch_url = format!(
                    "{}/wallet/{}",
                    ctx.rpc_url.to_string().trim_end_matches('/'),
                    watch_name
                );
                let client = Client::new(&watch_url, ctx.auth.clone()).expect("watch client");

                // Serialize our single-output OP_RETURN transaction to hex
                let raw_hex = hex::encode(bitcoin::consensus::serialize(&brc_tx));

                // Fund it from the watch-only wallet (select inputs, add change)
                let mut options = serde_json::json!({
                    "include_unsafe": true,
                    "change_type": "bech32m",
                });
                if let Some(fr) = fee_rate { options["fee_rate"] = serde_json::json!(*fr); }
                let funded: serde_json::Value = client.call(
                    "fundrawtransaction",
                    &[
                        serde_json::json!(raw_hex),
                        options,
                    ],
                )?;
                let funded_hex = funded["hex"].as_str().ok_or_else(|| anyhow::anyhow!("funded hex"))?;

                // Convert to PSBT and let Core fill UTXO/derivations
                let psbt_from_raw: serde_json::Value = client.call(
                    "converttopsbt",
                    &[
                        serde_json::json!(funded_hex),
                        serde_json::json!(false),
                        serde_json::json!(true),
                    ],
                )?;
                let psbt_b64 = psbt_from_raw.as_str().ok_or_else(|| anyhow::anyhow!("psbt tmp"))?;
                let mut psbt: Psbt = psbt_b64.parse()?;

                let finalized = wallet.sign(&mut psbt, passphrase.clone())?;
                let secp = bitcoin::secp256k1::Secp256k1::verification_only();
                if !finalized {
                    let psbt2 = psbt.clone();
                    let finalized_psbt = psbt2.finalize(&secp).map_err(|e| anyhow::anyhow!("finalize: {:?}", e))?;
                    psbt = finalized_psbt;
                }
                let tx = psbt.extract_tx().map_err(|e| anyhow::anyhow!("extract_tx: {e}"))?;
                // Broadcast using root client since wallet client may lack mempool policy overrides
                let txid = Client::new(ctx.rpc_url.as_ref(), ctx.auth.clone())?.send_raw_transaction(&tx)?;

                log::info!("✅ Registered collection {}, rebaseable: {}, txid: {}", laos_hex, rebaseable, txid);
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
