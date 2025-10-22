use super::CommandRunner;
use crate::cli;
use anyhow::{anyhow, Context, Result};
use bdk_wallet::signer::SignOptions;
use bitcoin::script::Builder as ScriptBuilder;
use bitcoin::{Amount, FeeRate, Transaction};
use bitcoincore_rpc::RpcApi;
use std::str::FromStr;

impl CommandRunner for cli::TxCmd {
    fn run(&self, cli: &cli::Cli) -> Result<()> {
        match self {
            cli::TxCmd::RegisterCollection {
                laos_hex,
                rebaseable,
                fee_rate,
            } => {
                let net = crate::network::parse_network(Some(cli.network.clone()));

                let collection_address = crate::types::CollectionAddress::from_str(laos_hex)
                    .context("parsing --laos-hex as 20-byte hex (H160)")?;

                let msg = crate::types::RegisterCollectionMessage {
                    collection_address,
                    rebaseable: *rebaseable,
                };
                let payload = msg.encode();

                let script = {
                    use bitcoin::blockdata::opcodes::all as opcodes;
                    let push = bitcoin::script::PushBytesBuf::try_from(payload.to_vec())
                        .map_err(|_| anyhow!("failed to create pushbytes for payload"))?;
                    ScriptBuilder::new()
                        .push_opcode(opcodes::OP_RETURN)
                        .push_opcode(crate::types::BRC721_CODE)
                        .push_slice(push)
                        .into_script()
                };

                let db_path = crate::wallet::paths::wallet_db_path(&cli.data_dir, net);
                let mut conn = rusqlite::Connection::open(&db_path)
                    .with_context(|| format!("opening wallet db at {}", db_path.display()))?;

                let mut wallet = bdk_wallet::LoadParams::new()
                    .check_network(net)
                    .load_wallet(&mut conn)?
                    .ok_or_else(|| anyhow!("wallet not initialized"))?;

                let mut builder = wallet.build_tx();
                builder.add_recipient(script, Amount::from_sat(0));
                if let Some(fr) = fee_rate {
                    let sats_vb = (*fr).max(0.0).round() as u64;
                    if let Some(fr) = FeeRate::from_sat_per_vb(sats_vb) {
                        builder.fee_rate(fr);
                    }
                }

                let mut psbt = builder.finish().context("building PSBT")?;

                let _all_signed = wallet
                    .sign(&mut psbt, SignOptions::default())
                    .context("signing PSBT with local wallet")?;

                let tx: Transaction = psbt.extract_tx()?;

                let _ = wallet.persist(&mut conn)?;

                let auth = match (&cli.rpc_user, &cli.rpc_pass) {
                    (Some(user), Some(pass)) => {
                        bitcoincore_rpc::Auth::UserPass(user.clone(), pass.clone())
                    }
                    _ => bitcoincore_rpc::Auth::None,
                };
                let rpc = bitcoincore_rpc::Client::new(&cli.rpc_url, auth)
                    .context("creating RPC client")?;

                let txid = rpc
                    .send_raw_transaction(&tx)
                    .context("broadcasting transaction via RPC")?;

                log::info!("{txid}");
                Ok(())
            }
        }
    }
}
