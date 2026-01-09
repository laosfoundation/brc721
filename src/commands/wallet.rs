use super::CommandRunner;
use crate::wallet::brc721_wallet::Brc721Wallet;
use crate::wallet::passphrase::prompt_passphrase;
use crate::{cli, context};
use age::secrecy::SecretString;
use anyhow::{anyhow, Context, Result};
use bdk_wallet::bip39::{Language, Mnemonic};
use serde::Serialize;
use rand::{rngs::OsRng, RngCore};
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

fn parse_slot_from_token_id(token_id: &str) -> Result<u128> {
    let value = ethereum_types::U256::from_dec_str(token_id.trim())
        .map_err(|err| anyhow!("invalid token id '{token_id}': {err}"))?;
    let token = crate::types::Brc721Token::try_from(value)
        .map_err(|err| anyhow!("invalid token id encoding '{token_id}': {err}"))?;
    Ok(token.slot_number())
}

fn merge_slots_to_ranges(mut slots: Vec<u128>) -> Vec<(u128, u128)> {
    slots.sort_unstable();
    slots.dedup();

    let mut ranges = Vec::new();
    let mut iter = slots.into_iter();
    let Some(mut start) = iter.next() else {
        return ranges;
    };
    let mut end = start;

    for slot in iter {
        if slot == end.saturating_add(1) {
            end = slot;
            continue;
        }
        ranges.push((start, end));
        start = slot;
        end = slot;
    }
    ranges.push((start, end));
    ranges
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
            println!("{}", serde_json::to_string_pretty(&WalletAssetsJson { results: vec![] })?);
        } else {
            log::info!(
                "📭 No scanner database found at {} (run the daemon to build an index)",
                db_path.to_string_lossy()
            );
        }
        return Ok(());
    }

    let wallet = load_wallet(ctx)?;
    let unspent = wallet
        .list_unspent(min_conf)
        .context("list wallet UTXOs")?;

    let storage = crate::storage::SqliteStorage::new(&db_path);

    let mut by_address: BTreeMap<String, (String, Vec<OwnershipUtxoJson>)> = BTreeMap::new();

    for utxo in unspent {
        let txid = utxo.txid.to_string();
        let vout = utxo.vout;

        let tokens = storage
            .list_registered_tokens_by_outpoint(&txid, vout)
            .with_context(|| format!("query registered tokens for {txid}:{vout}"))?;
        if tokens.is_empty() {
            continue;
        }

        let owner_h160 = {
            let hash = hash160::Hash::hash(utxo.script_pub_key.as_bytes());
            let h160 = ethereum_types::H160::from_slice(hash.as_byte_array());
            format!("{:#x}", h160)
        };

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

        let mut tokens_by_collection: BTreeMap<String, (u64, u32, Vec<(u128, String)>)> =
            BTreeMap::new();
        for token in tokens {
            let slot = match parse_slot_from_token_id(&token.token_id) {
                Ok(slot) => slot,
                Err(err) => {
                    log::warn!("Skipping invalid stored token id {}: {}", token.token_id, err);
                    continue;
                }
            };

            let entry = tokens_by_collection
                .entry(token.collection_id.to_string())
                .or_insert((token.created_height, token.created_tx_index, Vec::new()));
            entry.2.push((slot, token.token_id));
        }

        for (collection_id, (created_height, created_tx_index, mut tokens)) in tokens_by_collection
        {
            tokens.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            let slots = tokens.iter().map(|(slot, _)| *slot).collect::<Vec<_>>();
            let ranges = merge_slots_to_ranges(slots);
            let slot_ranges = ranges
                .iter()
                .map(|(start, end)| SlotRangeJson {
                    start: start.to_string(),
                    end: end.to_string(),
                })
                .collect::<Vec<_>>();

            let utxo_entry = OwnershipUtxoJson {
                collection_id,
                txid: txid.clone(),
                vout,
                created_height,
                created_tx_index,
                slot_ranges,
                asset_ids: asset_ids.then(|| tokens.into_iter().map(|(_, id)| id).collect()),
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
        log::info!("📭 No indexed BRC-721 assets found for this wallet (min_conf={})", min_conf);
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
    fn merge_slots_to_ranges_merges_consecutive_slots() {
        let ranges = merge_slots_to_ranges(vec![5, 3, 4, 4, 10]);
        assert_eq!(ranges, vec![(3, 5), (10, 10)]);
        assert_eq!(format_ranges(&ranges), "3..=5,10");
    }

    #[test]
    fn parse_slot_from_token_id_extracts_slot() {
        let token =
            crate::types::Brc721Token::new(42, ethereum_types::H160::from_low_u64_be(1)).unwrap();
        let token_id = token.to_u256().to_string();
        let slot = parse_slot_from_token_id(&token_id).unwrap();
        assert_eq!(slot, 42);
    }
}
