use super::CommandRunner;
use crate::wallet::brc721_wallet::Brc721Wallet;
use crate::wallet::passphrase::prompt_passphrase;
use crate::{cli, context};
use age::secrecy::SecretString;
use anyhow::{anyhow, Context, Result};
use bdk_wallet::bip39::{Language, Mnemonic};
use ethereum_types::H160;
use rand::{rngs::OsRng, RngCore};
use serde::Serialize;
use std::collections::BTreeMap;
use std::str::FromStr;

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
            } => run_init(ctx, mnemonic.clone(), passphrase.clone()),
            cli::WalletCmd::Generate { short } => run_generate(*short),
            cli::WalletCmd::Address => run_address(ctx),
            cli::WalletCmd::Balance => run_balance(ctx),
            cli::WalletCmd::Rescan => run_rescan(ctx),
            cli::WalletCmd::Info => run_info(ctx),
            cli::WalletCmd::Load => run_load(ctx),
            cli::WalletCmd::Unload => run_unload(ctx),
            cli::WalletCmd::Assets {
                min_conf,
                json,
                asset_ids,
            } => run_assets(ctx, *min_conf, *json, *asset_ids),
        }
    }
}

fn run_init(
    ctx: &context::Context,
    mnemonic: Option<String>,
    passphrase: Option<String>,
) -> Result<()> {
    // Check if wallet already exists
    if let Ok(wallet) = load_wallet(ctx) {
        wallet.setup_watch_only().context("setup watch only")?;
        log::info!("📡 Watch-only wallet '{}' ready in Core", wallet.id());
        return Ok(());
    }

    let mnemonic_str = mnemonic
        .as_ref()
        .ok_or_else(|| anyhow!("mnemonic is required when creating a new wallet"))?;

    let mnemonic =
        Mnemonic::parse_in(Language::English, mnemonic_str).context("invalid mnemonic")?;

    // Resolve passphrase
    let passphrase = resolve_passphrase_init(passphrase);

    // Create new wallet
    let wallet = Brc721Wallet::create(
        &ctx.data_dir,
        ctx.network,
        mnemonic,
        passphrase,
        &ctx.rpc_url,
        ctx.auth.clone(),
    )
    .context("wallet initialization")?;

    wallet.setup_watch_only().context("setup watch only")?;

    log::info!("🎉 New wallet created");
    log::info!("📡 Watch-only wallet '{}' ready in Core", wallet.id());
    Ok(())
}

fn run_address(ctx: &context::Context) -> Result<()> {
    let mut wallet = load_wallet(ctx)?;
    let addr = wallet
        .reveal_next_payment_address()
        .context("getting address")?;
    log::info!("🏠 {}", addr.address);
    Ok(())
}

fn run_balance(ctx: &context::Context) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    let balances = wallet.balances()?;
    log::info!("💰 {:?}", balances);
    Ok(())
}

fn run_rescan(ctx: &context::Context) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    wallet
        .rescan_watch_only()
        .context("rescan watch-only wallet")?;
    log::info!("🔄 Rescan started for watch-only wallet '{}'", wallet.id());
    Ok(())
}

fn run_info(ctx: &context::Context) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    let loaded_wallets = wallet.loaded_core_wallets().context("list Core wallets")?;

    log::info!("🆔 Local wallet id: {}", wallet.id());
    if loaded_wallets.is_empty() {
        log::info!("📂 Bitcoin Core has no wallets loaded");
    } else {
        log::info!(
            "📂 Bitcoin Core loaded wallets: {}",
            loaded_wallets.join(", ")
        );
    }
    Ok(())
}

fn run_load(ctx: &context::Context) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    wallet.load_watch_only().context("load watch-only wallet")?;
    log::info!("📡 Watch-only wallet '{}' loaded in Core", wallet.id());
    Ok(())
}

fn run_unload(ctx: &context::Context) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    wallet
        .unload_watch_only()
        .context("unload watch-only wallet")?;
    log::info!("🛑 Watch-only wallet '{}' unloaded from Core", wallet.id());
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SlotRangeJson {
    start: String,
    end: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnershipUtxoJson {
    collection_id: String,
    txid: String,
    vout: u32,
    created_height: u64,
    created_tx_index: u32,
    slot_ranges: Vec<SlotRangeJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    asset_ids: Option<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AddressAssetsJson {
    address: String,
    owner_h160: String,
    utxos: Vec<OwnershipUtxoJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletAssetsJson {
    results: Vec<AddressAssetsJson>,
}

fn merge_ranges(mut ranges: Vec<(u128, u128)>) -> Vec<(u128, u128)> {
    ranges.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut out = Vec::new();
    for (start, end) in ranges {
        let Some(last) = out.last_mut() else {
            out.push((start, end));
            continue;
        };

        if start <= last.1.saturating_add(1) {
            last.1 = last.1.max(end);
            continue;
        }

        out.push((start, end));
    }
    out
}

fn token_id_decimal(slot_number: u128, base_h160: H160) -> String {
    match crate::types::Brc721Token::new(slot_number, base_h160) {
        Ok(token) => token.to_u256().to_string(),
        Err(_) => format!("<invalid slot {}>", slot_number),
    }
}

fn asset_ids_for_ranges(ranges: &[(u128, u128)], base_h160: H160) -> Vec<String> {
    const MAX_ASSET_IDS_PER_UTXO: u128 = 32;

    let mut total_count = 0u128;
    let mut emitted_count = 0u128;
    let mut assets = Vec::new();

    for (start, end) in ranges {
        let count = end.saturating_sub(*start).saturating_add(1);
        total_count = total_count.saturating_add(count);

        if emitted_count >= MAX_ASSET_IDS_PER_UTXO {
            continue;
        }

        let mut slot = *start;
        loop {
            if emitted_count >= MAX_ASSET_IDS_PER_UTXO {
                break;
            }
            assets.push(token_id_decimal(slot, base_h160));
            emitted_count += 1;

            if slot == *end {
                break;
            }
            slot = slot.saturating_add(1);
        }
    }

    if emitted_count < total_count {
        assets.push(format!("...+{} more", total_count - emitted_count));
    }

    assets
}

fn format_ranges(ranges: &[(u128, u128)]) -> String {
    ranges
        .iter()
        .map(|(start, end)| {
            if start == end {
                start.to_string()
            } else {
                format!("{start}..={end}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn run_assets(ctx: &context::Context, min_conf: u64, json: bool, asset_ids: bool) -> Result<()> {
    use bitcoin::hashes::{hash160, Hash};

    let db_path = ctx.data_dir.join("brc721.sqlite");
    if !db_path.exists() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&WalletAssetsJson { results: vec![] })?
            );
        } else {
            log::info!(
                "📭 No scanner database found at {} (run the daemon to build an index)",
                db_path.to_string_lossy()
            );
        }
        return Ok(());
    }

    let wallet = load_wallet(ctx)?;
    let unspent = wallet.list_unspent(min_conf).context("list wallet UTXOs")?;

    let storage = crate::storage::SqliteStorage::new(&db_path);

    let mut by_address: BTreeMap<String, (String, Vec<OwnershipUtxoJson>)> = BTreeMap::new();

    for utxo in unspent {
        let txid = utxo.txid.to_string();
        let vout = utxo.vout;

        let Some((ownership_utxo, ranges)) = storage
            .load_ownership_utxo_with_ranges_by_outpoint(&txid, vout)
            .with_context(|| format!("query ownership ranges for {txid}:{vout}"))?
        else {
            continue;
        };

        if let Some(spent_txid) = ownership_utxo.spent_txid.as_deref() {
            log::warn!(
                "Skipping ownership outpoint {}:{} marked spent in DB (spent_by={})",
                txid,
                vout,
                spent_txid
            );
            continue;
        }

        let (owner_h160, owner_h160_raw) = {
            let hash = hash160::Hash::hash(utxo.script_pub_key.as_bytes());
            let h160 = H160::from_slice(hash.as_byte_array());
            (format!("{:#x}", h160), h160)
        };

        if owner_h160_raw != ownership_utxo.owner_h160 {
            log::warn!(
                "Outpoint {}:{} ownerH160 mismatch (wallet={} db={:#x})",
                txid,
                vout,
                owner_h160,
                ownership_utxo.owner_h160
            );
        }

        let address = bitcoin::Address::from_script(&utxo.script_pub_key, ctx.network)
            .ok()
            .map(|address| address.to_string())
            .or_else(|| {
                utxo.address
                    .as_ref()
                    .and_then(|address| address.clone().require_network(ctx.network).ok())
                    .map(|address| address.to_string())
            })
            .unwrap_or_else(|| {
                format!("<unknown:{}>", hex::encode(utxo.script_pub_key.as_bytes()))
            });

        let ranges = merge_ranges(
            ranges
                .iter()
                .map(|range| (range.slot_start, range.slot_end))
                .collect(),
        );

        let slot_ranges = ranges
            .iter()
            .map(|(start, end)| SlotRangeJson {
                start: start.to_string(),
                end: end.to_string(),
            })
            .collect::<Vec<_>>();

        let utxo_entry = OwnershipUtxoJson {
            collection_id: ownership_utxo.collection_id.to_string(),
            txid: txid.clone(),
            vout,
            created_height: ownership_utxo.created_height,
            created_tx_index: ownership_utxo.created_tx_index,
            slot_ranges,
            asset_ids: asset_ids.then(|| asset_ids_for_ranges(&ranges, ownership_utxo.base_h160)),
        };

        let addr_entry = by_address
            .entry(address.clone())
            .or_insert_with(|| (owner_h160.clone(), Vec::new()));

        if addr_entry.0 != owner_h160 {
            log::warn!(
                "Address {} has inconsistent ownerH160 values: {} vs {}",
                address,
                addr_entry.0,
                owner_h160
            );
        }

        addr_entry.1.push(utxo_entry);
    }

    let results = by_address
        .into_iter()
        .map(|(address, (owner_h160, mut utxos))| {
            utxos.sort_by(|a, b| {
                a.collection_id
                    .cmp(&b.collection_id)
                    .then_with(|| a.txid.cmp(&b.txid))
                    .then_with(|| a.vout.cmp(&b.vout))
            });
            AddressAssetsJson {
                address,
                owner_h160,
                utxos,
            }
        })
        .collect::<Vec<_>>();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&WalletAssetsJson { results })?
        );
        return Ok(());
    }

    if results.is_empty() {
        log::info!(
            "📭 No indexed BRC-721 assets found for this wallet (min_conf={})",
            min_conf
        );
        return Ok(());
    }

    log::info!(
        "🎨 Indexed BRC-721 assets (wallet_id={}, min_conf={})",
        wallet.id(),
        min_conf
    );

    for entry in results {
        log::info!("🏠 {} (ownerH160={})", entry.address, entry.owner_h160);
        for utxo in entry.utxos {
            let ranges = utxo
                .slot_ranges
                .iter()
                .filter_map(|r| {
                    let start = u128::from_str(&r.start).ok()?;
                    let end = u128::from_str(&r.end).ok()?;
                    Some((start, end))
                })
                .collect::<Vec<_>>();
            log::info!(
                "  - collection={} outpoint={}:{} created={}#{} slots={}",
                utxo.collection_id,
                utxo.txid,
                utxo.vout,
                utxo.created_height,
                utxo.created_tx_index,
                format_ranges(&ranges)
            );
            if let Some(ids) = &utxo.asset_ids {
                log::info!("    asset_ids=[{}]", ids.join(","));
            }
        }
    }

    Ok(())
}

fn load_wallet(ctx: &context::Context) -> Result<Brc721Wallet> {
    Brc721Wallet::load(&ctx.data_dir, ctx.network, &ctx.rpc_url, ctx.auth.clone())
        .context("loading wallet")
}

fn resolve_passphrase_init(passphrase: Option<String>) -> SecretString {
    passphrase.map(SecretString::from).unwrap_or_else(|| {
        SecretString::from(prompt_passphrase().expect("prompt").unwrap_or_default())
    })
}

fn generate_mnemonic(short: bool) -> Mnemonic {
    let entropy_bytes = if short { 16 } else { 32 };
    let mut entropy = vec![0u8; entropy_bytes];
    OsRng.fill_bytes(&mut entropy);
    Mnemonic::from_entropy(&entropy).expect("mnemonic")
}

fn run_generate(short: bool) -> Result<()> {
    let mnemonic = generate_mnemonic(short);
    println!("{}", mnemonic);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_mnemonic_is_valid_12_words() {
        let mnemonic = generate_mnemonic(true);
        assert_eq!(mnemonic.word_count(), 12);
        let parsed = Mnemonic::parse_in(Language::English, mnemonic.to_string()).expect("parse");
        assert_eq!(parsed.word_count(), 12);
    }

    #[test]
    fn generate_mnemonic_is_valid_24_words() {
        let mnemonic = generate_mnemonic(false);
        assert_eq!(mnemonic.word_count(), 24);
        let parsed = Mnemonic::parse_in(Language::English, mnemonic.to_string()).expect("parse");
        assert_eq!(parsed.word_count(), 24);
    }

    #[test]
    fn merge_ranges_merges_consecutive_slots() {
        let ranges = merge_ranges(vec![(5, 5), (3, 4), (4, 4), (10, 10)]);
        assert_eq!(ranges, vec![(3, 5), (10, 10)]);
        assert_eq!(format_ranges(&ranges), "3..=5,10");
    }

    #[test]
    fn token_id_decimal_matches_brc721_token_encoding() {
        let base_h160 = ethereum_types::H160::from_low_u64_be(1);
        let token = crate::types::Brc721Token::new(42, base_h160).unwrap();
        assert_eq!(token_id_decimal(42, base_h160), token.to_u256().to_string());
    }
}
