use std::str::FromStr;

use super::CommandRunner;
use crate::cli::OwnershipAssignmentArg;
use crate::storage::traits::CollectionKey;
use crate::types::{
    Brc721Message, Brc721Output, OwnershipGroup, RegisterCollectionData, RegisterOwnershipData,
};
use crate::wallet::passphrase::prompt_passphrase_once;
use crate::{cli, context, wallet::brc721_wallet::Brc721Wallet};
use age::secrecy::SecretString;
use anyhow::{anyhow, Context, Result};
use bitcoin::{Address, Amount, TxOut};
use ethereum_types::H160;

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
            cli::TxCmd::RegisterOwnership {
                collection_id,
                assignments,
                fee_rate,
                passphrase,
            } => run_register_ownership(
                ctx,
                collection_id,
                assignments,
                *fee_rate,
                passphrase.clone(),
            ),
            cli::TxCmd::SendAmount {
                to,
                amount_sat,
                fee_rate,
                passphrase,
            } => run_send_amount(ctx, to, *amount_sat, *fee_rate, passphrase.clone()),
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
    let msg = Brc721Message::RegisterCollection(msg);
    let output = Brc721Output::new(msg).into_txout().unwrap();

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
    assignments: &[OwnershipAssignmentArg],
    fee_rate: Option<f64>,
    passphrase: Option<String>,
) -> Result<()> {
    if assignments.is_empty() {
        return Err(anyhow!("at least one ownership assignment is required"));
    }
    if assignments.len() > u8::MAX as usize {
        return Err(anyhow!(
            "assignment count {} exceeds protocol limit of {}",
            assignments.len(),
            u8::MAX
        ));
    }

    let wallet = load_wallet(ctx)?;

    let mut groups = Vec::with_capacity(assignments.len());
    let mut outputs = Vec::with_capacity(assignments.len() + 1);

    for (idx, assignment) in assignments.iter().enumerate() {
        if assignment.slot_ranges.len() > u8::MAX as usize {
            return Err(anyhow!(
                "assignment {} has {} slot ranges, exceeding limit of {}",
                idx + 1,
                assignment.slot_ranges.len(),
                u8::MAX
            ));
        }

        groups.push(OwnershipGroup {
            output_index: (idx + 1) as u8,
            slot_ranges: assignment.slot_ranges.clone(),
        });
    }

    let ownership_payload = RegisterOwnershipData {
        collection_block_height: collection_id.block_height,
        collection_tx_index: collection_id.tx_index,
        groups,
    };

    let op_return_output =
        Brc721Output::new(Brc721Message::RegisterOwnership(ownership_payload.clone()))
            .into_txout()
            .context("build ownership OP_RETURN output")?;
    outputs.push(op_return_output);

    for assignment in assignments {
        let address = Address::from_str(&assignment.address)?.require_network(ctx.network)?;
        outputs.push(TxOut {
            value: Amount::from_sat(assignment.amount_sat),
            script_pubkey: address.script_pubkey(),
        });
    }

    let passphrase = resolve_passphrase(passphrase);
    let tx = wallet
        .build_custom_tx(outputs, fee_rate, passphrase)
        .context("build ownership tx")?;
    let txid = wallet.broadcast(&tx)?;

    log::info!(
        "✅ Registered ownership for collection {} ({} assignments) txid: {}",
        collection_id,
        assignments.len(),
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
