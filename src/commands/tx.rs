use std::str::FromStr;

use super::CommandRunner;
use crate::storage::traits::{CollectionKey, StorageRead};
use crate::types::{
    Brc721OpReturnOutput, Brc721Payload, IndexRanges, MixData, RegisterCollectionData,
    RegisterOwnershipData, SlotRanges,
};
use crate::wallet::passphrase::prompt_passphrase_once;
use crate::{cli, context, wallet::brc721_wallet::Brc721Wallet};
use age::secrecy::SecretString;
use anyhow::{anyhow, Context, Result};
use bitcoin::{Address, Amount, OutPoint};
use ethereum_types::H160;
use std::collections::BTreeSet;

impl CommandRunner for cli::TxCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        match self {
            cli::TxCmd::RegisterCollection {
                evm_collection_address,
                rebaseable,
                fee_rate,
                passphrase,
            } => run_register_collection(
                ctx,
                *evm_collection_address,
                *rebaseable,
                *fee_rate,
                passphrase.clone(),
            ),
            cli::TxCmd::SendAmount {
                to,
                amount_sat,
                fee_rate,
                passphrase,
            } => run_send_amount(ctx, to, *amount_sat, *fee_rate, passphrase.clone()),
            cli::TxCmd::RegisterOwnership {
                collection_id,
                slots,
                fee_rate,
                passphrase,
            } => run_register_ownership(
                ctx,
                collection_id,
                slots.clone(),
                *fee_rate,
                passphrase.clone(),
            ),
            cli::TxCmd::SendAssets {
                to,
                outpoints,
                dust_sat,
                fee_rate,
                passphrase,
            } => run_send_assets(ctx, to, outpoints, *dust_sat, *fee_rate, passphrase.clone()),
            cli::TxCmd::Mix {
                outpoints,
                outputs,
                dust_sat,
                fee_rate,
                passphrase,
            } => run_mix(
                ctx,
                outpoints,
                outputs,
                *dust_sat,
                *fee_rate,
                passphrase.clone(),
            ),
        }
    }
}

fn run_register_collection(
    ctx: &context::Context,
    evm_collection_address: H160,
    rebaseable: bool,
    fee_rate: Option<f64>,
    passphrase: Option<String>,
) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    let msg = RegisterCollectionData {
        evm_collection_address,
        rebaseable,
    };
    let payload = Brc721Payload::RegisterCollection(msg);
    let output = Brc721OpReturnOutput::new(payload).into_txout().unwrap();

    let passphrase = resolve_passphrase(passphrase);
    let tx = wallet
        .build_tx(output, fee_rate, passphrase)
        .context("build tx")?;
    let txid = wallet.broadcast(&tx)?;

    log::info!(
        "✅ Registered collection {:#x}, rebaseable: {}, txid: {}",
        evm_collection_address,
        rebaseable,
        txid
    );
    Ok(())
}

fn run_register_ownership(
    ctx: &context::Context,
    collection_id: &CollectionKey,
    slots: SlotRanges,
    fee_rate: Option<f64>,
    passphrase: Option<String>,
) -> Result<()> {
    let mut wallet = load_wallet(ctx)?;

    // Output 1 is the ownership UTXO tracked by the indexer for this registration.
    // Use a new wallet-derived address so the NFTs are spendable by this wallet.
    let ownership_address = wallet
        .reveal_next_payment_address()
        .context("derive ownership address")?
        .address;
    let ownership_amount = Amount::from_sat(546);

    let ownership = RegisterOwnershipData::for_single_output(
        collection_id.block_height,
        collection_id.tx_index,
        slots,
    )?;
    let payload = Brc721Payload::RegisterOwnership(ownership);

    let output = Brc721OpReturnOutput::new(payload).into_txout().unwrap();

    let passphrase = resolve_passphrase(passphrase);
    let tx = wallet
        .build_tx_with_op_return_and_payments(
            output,
            vec![(ownership_address.clone(), ownership_amount)],
            fee_rate,
            passphrase,
        )
        .context("build tx")?;
    let txid = wallet.broadcast(&tx)?;

    log::info!(
        "✅ Registered ownership for collection {} (cmd=0x01), owner_output={}, txid: {}",
        collection_id,
        ownership_address,
        txid
    );
    Ok(())
}

fn run_send_amount(
    ctx: &context::Context,
    to: &str,
    amount_sat: u64,
    fee_rate: Option<f64>,
    passphrase: Option<String>,
) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    let amount = Amount::from_sat(amount_sat);
    let address = Address::from_str(to)?.require_network(ctx.network)?;
    let passphrase = resolve_passphrase(passphrase);
    let tx = wallet
        .build_payment_tx(&address, amount, fee_rate, passphrase)
        .context("build payment tx")?;
    let txid = wallet.broadcast(&tx)?;
    log::info!("✅ Sent {} sat to {} (txid: {})", amount_sat, to, txid);
    Ok(())
}

fn run_send_assets(
    ctx: &context::Context,
    to: &str,
    outpoints: &[String],
    dust_sat: u64,
    fee_rate: Option<f64>,
    passphrase: Option<String>,
) -> Result<()> {
    let db_path = ctx.data_dir.join("brc721.sqlite");
    if !db_path.exists() {
        return Err(anyhow!(
            "scanner database not found at {} (run the daemon to build an index)",
            db_path.to_string_lossy()
        ));
    }

    let token_outpoints = parse_outpoints(outpoints)?;
    if token_outpoints.is_empty() {
        return Err(anyhow!("at least one --outpoint is required"));
    }
    let unique = token_outpoints.iter().cloned().collect::<BTreeSet<_>>();
    if unique.len() != token_outpoints.len() {
        return Err(anyhow!("duplicate --outpoint provided"));
    }

    let storage = crate::storage::SqliteStorage::new(&db_path);

    for outpoint in &token_outpoints {
        let groups = storage
            .list_unspent_ownership_utxos_by_outpoint(&outpoint.txid.to_string(), outpoint.vout)
            .with_context(|| {
                format!(
                    "query ownership ranges for {}:{}",
                    outpoint.txid, outpoint.vout
                )
            })?;
        if groups.is_empty() {
            return Err(anyhow!(
                "outpoint {}:{} not found in the BRC-721 index (not an ownership UTXO, or not scanned yet)",
                outpoint.txid,
                outpoint.vout
            ));
        }
    }

    let wallet = load_wallet(ctx)?;
    let wallet_utxos = wallet.list_unspent(0).context("list wallet UTXOs")?;

    let wallet_outpoints = wallet_utxos
        .iter()
        .map(|utxo| OutPoint {
            txid: utxo.txid,
            vout: utxo.vout,
        })
        .collect::<BTreeSet<_>>();

    for outpoint in &token_outpoints {
        if !wallet_outpoints.contains(outpoint) {
            return Err(anyhow!(
                "outpoint {}:{} is not spendable by this wallet",
                outpoint.txid,
                outpoint.vout
            ));
        }
    }

    let address = Address::from_str(to)?.require_network(ctx.network)?;
    let dust_amount = Amount::from_sat(dust_sat);
    let passphrase = resolve_passphrase(passphrase);

    let lock_outpoints =
        compute_wallet_token_outpoints_to_lock(&storage, &wallet_utxos, &token_outpoints)
            .context("compute lock set")?;

    let tx = wallet
        .build_implicit_transfer_tx(
            &token_outpoints,
            &address,
            dust_amount,
            fee_rate,
            &lock_outpoints,
            passphrase,
        )
        .context("build implicit transfer tx")?;

    let txid = wallet.broadcast(&tx)?;
    log::info!(
        "✅ Sent {} ownership outpoint(s) to {} via implicit transfer (txid: {})",
        token_outpoints.len(),
        to,
        txid
    );
    log::info!(
        "ℹ️ Ownership changes are reflected by the scanner after {} confirmation(s).",
        ctx.confirmations
    );
    log::info!(
        "   Until then, the sender/receiver asset views may still show the previous ownership state."
    );
    log::info!("   Conflicting transfers of the same outpoint(s) can only resolve once one transaction confirms.");
    Ok(())
}

fn run_mix(
    ctx: &context::Context,
    outpoints: &[String],
    outputs: &[String],
    dust_sat: u64,
    fee_rate: Option<f64>,
    passphrase: Option<String>,
) -> Result<()> {
    let db_path = ctx.data_dir.join("brc721.sqlite");
    if !db_path.exists() {
        return Err(anyhow!(
            "scanner database not found at {} (run the daemon to build an index)",
            db_path.to_string_lossy()
        ));
    }

    let token_outpoints = parse_outpoints(outpoints)?;
    if token_outpoints.is_empty() {
        return Err(anyhow!("at least one --outpoint is required"));
    }
    let unique = token_outpoints.iter().cloned().collect::<BTreeSet<_>>();
    if unique.len() != token_outpoints.len() {
        return Err(anyhow!("duplicate --outpoint provided"));
    }

    let (output_addresses, mix_data) = parse_mix_outputs(outputs, ctx.network)?;

    let storage = crate::storage::SqliteStorage::new(&db_path);

    let mut total_tokens = 0u128;
    for outpoint in &token_outpoints {
        let groups = storage
            .load_unspent_ownership_utxos_with_ranges_by_outpoint(
                &outpoint.txid.to_string(),
                outpoint.vout,
            )
            .with_context(|| {
                format!(
                    "query ownership ranges for {}:{}",
                    outpoint.txid, outpoint.vout
                )
            })?;
        if groups.is_empty() {
            return Err(anyhow!(
                "outpoint {}:{} not found in the BRC-721 index (not an ownership UTXO, or not scanned yet)",
                outpoint.txid,
                outpoint.vout
            ));
        }

        for (_utxo, ranges) in groups {
            for range in ranges {
                let len = range
                    .slot_end
                    .checked_sub(range.slot_start)
                    .and_then(|delta| delta.checked_add(1))
                    .ok_or_else(|| anyhow!("token range length overflow"))?;
                total_tokens = total_tokens
                    .checked_add(len)
                    .ok_or_else(|| anyhow!("token count overflow"))?;
            }
        }
    }

    mix_data.validate_token_count(total_tokens)?;

    let wallet = load_wallet(ctx)?;
    let wallet_utxos = wallet.list_unspent(0).context("list wallet UTXOs")?;

    let wallet_outpoints = wallet_utxos
        .iter()
        .map(|utxo| OutPoint {
            txid: utxo.txid,
            vout: utxo.vout,
        })
        .collect::<BTreeSet<_>>();

    for outpoint in &token_outpoints {
        if !wallet_outpoints.contains(outpoint) {
            return Err(anyhow!(
                "outpoint {}:{} is not spendable by this wallet",
                outpoint.txid,
                outpoint.vout
            ));
        }
    }

    let dust_amount = Amount::from_sat(dust_sat);
    let payments = output_addresses
        .into_iter()
        .map(|address| (address, dust_amount))
        .collect::<Vec<_>>();
    let output_count = payments.len();

    let op_return = Brc721OpReturnOutput::new(Brc721Payload::Mix(mix_data))
        .into_txout()
        .context("build mix op_return output")?;

    let passphrase = resolve_passphrase(passphrase);
    let tx = wallet
        .build_mix_tx(&token_outpoints, op_return, payments, fee_rate, passphrase)
        .context("build mix tx")?;

    let txid = wallet.broadcast(&tx)?;
    log::info!(
        "✅ Mixed {} ownership outpoint(s) into {} output(s) (txid: {})",
        token_outpoints.len(),
        output_count,
        txid
    );
    log::info!(
        "ℹ️ Ownership changes are reflected by the scanner after {} confirmation(s).",
        ctx.confirmations
    );
    Ok(())
}

fn load_wallet(ctx: &context::Context) -> Result<Brc721Wallet> {
    Brc721Wallet::load(&ctx.data_dir, ctx.network, &ctx.rpc_url, ctx.auth.clone())
}

fn resolve_passphrase(passphrase: Option<String>) -> SecretString {
    passphrase.map(SecretString::from).unwrap_or_else(|| {
        SecretString::from(
            prompt_passphrase_once()
                .expect("prompt")
                .unwrap_or_default(),
        )
    })
}

fn parse_outpoints(outpoints: &[String]) -> Result<Vec<OutPoint>> {
    outpoints
        .iter()
        .map(|outpoint| {
            OutPoint::from_str(outpoint)
                .with_context(|| format!("invalid outpoint '{outpoint}' (expected TXID:VOUT)"))
        })
        .collect()
}

fn compute_wallet_token_outpoints_to_lock(
    storage: &crate::storage::SqliteStorage,
    wallet_utxos: &[bitcoincore_rpc::json::ListUnspentResultEntry],
    spending: &[OutPoint],
) -> Result<Vec<OutPoint>> {
    let spending_set = spending.iter().cloned().collect::<BTreeSet<_>>();

    // Build a set of wallet-owned outpoints that the index considers ownership UTXOs.
    let mut wallet_token_outpoints = BTreeSet::new();
    for utxo in wallet_utxos {
        let txid = utxo.txid.to_string();
        let vout = utxo.vout;
        if storage
            .list_unspent_ownership_utxos_by_outpoint(&txid, vout)
            .with_context(|| format!("query ownership ranges for {txid}:{vout}"))?
            .is_empty()
        {
            continue;
        }

        wallet_token_outpoints.insert(OutPoint {
            txid: utxo.txid,
            vout,
        });
    }

    Ok(wallet_token_outpoints
        .difference(&spending_set)
        .cloned()
        .collect())
}

fn parse_mix_outputs(
    outputs: &[String],
    network: bitcoin::Network,
) -> Result<(Vec<Address>, MixData)> {
    if outputs.is_empty() {
        return Err(anyhow!("mix requires at least one --output entry"));
    }

    let mut addresses = Vec::with_capacity(outputs.len());
    let mut ranges = Vec::with_capacity(outputs.len());
    let mut complement_index: Option<usize> = None;

    for (index, output) in outputs.iter().enumerate() {
        let (address_str, range_str) = output.split_once(':').ok_or_else(|| {
            anyhow!("invalid output '{output}' (expected ADDRESS:RANGES or ADDRESS:complement)")
        })?;

        let address = Address::from_str(address_str)?.require_network(network)?;

        if range_str.eq_ignore_ascii_case("complement")
            || range_str.eq_ignore_ascii_case("rest")
            || range_str == "*"
        {
            if complement_index.is_some() {
                return Err(anyhow!(
                    "mix requires exactly one complement output (duplicate at index {})",
                    index + 1
                ));
            }
            complement_index = Some(index);
            addresses.push(address);
            ranges.push(Vec::new());
            continue;
        }

        let parsed = IndexRanges::from_str(range_str)
            .map_err(|err| anyhow!("invalid output ranges '{range_str}': {err}"))?;
        addresses.push(address);
        ranges.push(parsed.into_ranges());
    }

    let Some(complement_index) = complement_index else {
        return Err(anyhow!(
            "mix requires exactly one complement output (use ADDRESS:complement)"
        ));
    };

    let data = MixData::new(ranges, complement_index)?;
    Ok((addresses, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_outpoints_rejects_invalid() {
        let res = parse_outpoints(&["not-an-outpoint".to_string()]);
        assert!(res.is_err());
    }
}
