use super::CommandRunner;
use crate::storage::traits::{OwnershipRange, StorageRead};
use crate::storage::SqliteStorage;
use crate::wallet::brc721_wallet::Brc721Wallet;
use crate::wallet::passphrase::prompt_passphrase;
use crate::{cli, context};
use age::secrecy::SecretString;
use anyhow::{anyhow, Context, Result};
use bdk_wallet::bip39::{Language, Mnemonic};
use bitcoin::hashes::{hash160, Hash as _};
use ethereum_types::H160;
use rand::{rngs::OsRng, RngCore};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
            cli::WalletCmd::Assets { min_conf, json } => run_assets(ctx, *min_conf, *json),
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
struct WalletAssetsResponse {
    pub wallet_id: String,
    pub min_conf: usize,
    pub owners: Vec<WalletOwnerAssets>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletOwnerAssets {
    pub address: String,
    pub owner_h160: String,
    pub utxos: Vec<WalletOwnershipUtxo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletOwnershipUtxo {
    pub collection_id: String,
    pub txid: String,
    pub vout: u32,
    pub created_height: u64,
    pub created_tx_index: u32,
    pub slot_ranges: Vec<WalletSlotRange>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletSlotRange {
    #[serde(serialize_with = "serialize_u128_as_string")]
    pub start: u128,
    #[serde(serialize_with = "serialize_u128_as_string")]
    pub end: u128,
}

fn serialize_u128_as_string<S>(value: &u128, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_string())
}

fn run_assets(ctx: &context::Context, min_conf: usize, json: bool) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    let wallet_id = wallet.id();

    let unspent = wallet
        .list_unspent(min_conf)
        .context("list wallet UTXOs (listunspent)")?;

    let mut owner_to_address: HashMap<H160, String> = HashMap::new();
    let mut owners: BTreeSet<H160> = BTreeSet::new();

    for entry in unspent {
        let script_hash = hash160::Hash::hash(entry.script_pub_key.as_bytes());
        let owner_h160 = H160::from_slice(script_hash.as_byte_array());
        owners.insert(owner_h160);

        let address = entry
            .address
            .and_then(|addr| addr.require_network(ctx.network).ok())
            .map(|addr| addr.to_string())
            .or_else(|| {
                bitcoin::Address::from_script(&entry.script_pub_key, ctx.network)
                    .map(|a| a.to_string())
                    .ok()
            })
            .unwrap_or_else(|| format!("script:{}", hex::encode(entry.script_pub_key.as_bytes())));

        owner_to_address.entry(owner_h160).or_insert(address);
    }

    let storage = SqliteStorage::new(ctx.data_dir.join("brc721.sqlite"));
    storage.init().context("open brc721 sqlite db")?;

    let owners_vec = owners.into_iter().collect::<Vec<_>>();
    let all_ranges = storage
        .list_unspent_ownership_by_owners(&owners_vec)
        .context("query ownership ranges")?;

    let mut by_owner: BTreeMap<H160, Vec<OwnershipRange>> = BTreeMap::new();
    for range in all_ranges {
        by_owner.entry(range.owner_h160).or_default().push(range);
    }

    let mut owner_assets = Vec::new();
    for (owner_h160, mut ranges) in by_owner {
        ranges.sort_by(|a, b| {
            a.collection_id
                .block_height
                .cmp(&b.collection_id.block_height)
                .then(a.collection_id.tx_index.cmp(&b.collection_id.tx_index))
                .then(a.outpoint.txid.cmp(&b.outpoint.txid))
                .then(a.outpoint.vout.cmp(&b.outpoint.vout))
                .then(a.slot_start.cmp(&b.slot_start))
                .then(a.slot_end.cmp(&b.slot_end))
        });

        let address = owner_to_address
            .get(&owner_h160)
            .cloned()
            .unwrap_or_else(|| format!("{:#x}", owner_h160));

        owner_assets.push(WalletOwnerAssets {
            address,
            owner_h160: format!("{:#x}", owner_h160),
            utxos: group_ownership_ranges(ranges),
        });
    }

    if json {
        let payload = WalletAssetsResponse {
            wallet_id,
            min_conf,
            owners: owner_assets,
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    if owner_assets.is_empty() {
        log::info!("📦 Wallet assets: none found (wallet_id={})", wallet_id);
        return Ok(());
    }

    log::info!("📦 Wallet assets (wallet_id={}):", wallet_id);
    for owner in owner_assets {
        log::info!("🏠 {} (owner_h160={})", owner.address, owner.owner_h160);
        for utxo in owner.utxos {
            let slot_ranges = utxo
                .slot_ranges
                .iter()
                .map(|r| {
                    if r.start == r.end {
                        r.start.to_string()
                    } else {
                        format!("{}..={}", r.start, r.end)
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            log::info!(
                "  - {}:{} collection={} created={}:{} slots={}",
                utxo.txid,
                utxo.vout,
                utxo.collection_id,
                utxo.created_height,
                utxo.created_tx_index,
                slot_ranges
            );
        }
    }

    Ok(())
}

fn group_ownership_ranges(ranges: Vec<OwnershipRange>) -> Vec<WalletOwnershipUtxo> {
    let mut out: BTreeMap<(String, bitcoin::Txid, u32), WalletOwnershipUtxo> = BTreeMap::new();

    for range in ranges {
        let key = (
            range.collection_id.to_string(),
            range.outpoint.txid,
            range.outpoint.vout,
        );
        let entry = out
            .entry(key.clone())
            .or_insert_with(|| WalletOwnershipUtxo {
                collection_id: key.0.clone(),
                txid: key.1.to_string(),
                vout: key.2,
                created_height: range.created_height,
                created_tx_index: range.created_tx_index,
                slot_ranges: Vec::new(),
            });

        entry.slot_ranges.push(WalletSlotRange {
            start: range.slot_start,
            end: range.slot_end,
        });
    }

    for utxo in out.values_mut() {
        utxo.slot_ranges
            .sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    }

    out.into_values().collect()
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
}
