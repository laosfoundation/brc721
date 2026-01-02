use std::str::FromStr;

use super::CommandRunner;
use crate::storage::traits::CollectionKey;
use crate::types::{
    Brc721OpReturnOutput, Brc721Payload, RegisterCollectionData, RegisterOwnershipData, SlotRanges,
};
use crate::wallet::passphrase::prompt_passphrase_once;
use crate::{cli, context, wallet::brc721_wallet::Brc721Wallet};
use age::secrecy::SecretString;
use anyhow::{Context, Result};
use bitcoin::{Address, Amount};
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
