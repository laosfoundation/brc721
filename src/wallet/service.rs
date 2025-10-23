use anyhow::{anyhow, Context, Result};
use bdk_wallet::{
    keys::bip39::{Language, Mnemonic, WordCount},
    template::Bip86,
    CreateParams, KeychainKind, LoadParams,
};
use bitcoin::{Amount, Network};
use rusqlite::Connection;

use super::paths::wallet_db_path;
use super::types::InitResult;

pub fn init_wallet(
    data_dir: &str,
    network: Network,
    mnemonic_str: Option<String>,
    passphrase: Option<String>,
) -> Result<InitResult> {
    let db_path = wallet_db_path(data_dir, network);
    let mut conn = Connection::open(&db_path)?;

    if let Some(_wallet) = LoadParams::new()
        .check_network(network)
        .load_wallet(&mut conn)?
    {
        return Ok(InitResult {
            created: false,
            mnemonic: None,
            db_path,
        });
    }

    let mnemonic = match mnemonic_str {
        Some(s) => Mnemonic::parse(s)?,
        None => {
            <Mnemonic as bdk_wallet::keys::GeneratableKey<bdk_wallet::miniscript::Tap>>::generate((
                WordCount::Words12,
                Language::English,
            ))
            .map_err(|e| {
                e.map(Into::into)
                    .unwrap_or_else(|| anyhow!("failed to generate mnemonic"))
            })?
            .into_key()
        }
    };

    let ext = Bip86(
        (mnemonic.clone(), passphrase.clone()),
        KeychainKind::External,
    );
    let int = Bip86((mnemonic.clone(), passphrase), KeychainKind::Internal);

    let _wallet = CreateParams::new(ext, int)
        .network(network)
        .create_wallet(&mut conn)?;

    Ok(InitResult {
        created: true,
        mnemonic: Some(mnemonic),
        db_path,
    })
}

pub fn load_public_descriptors_with_checksum(
    data_dir: &str,
    network: Network,
) -> Result<(String, String)> {
    let db_path = wallet_db_path(data_dir, network);
    let mut conn = Connection::open(&db_path)
        .with_context(|| format!("opening wallet db at {}", db_path.display()))?;

    let wallet = LoadParams::new()
        .check_network(network)
        .load_wallet(&mut conn)?
        .ok_or_else(|| anyhow!("wallet not initialized"))?;

    let ext_desc = wallet
        .public_descriptor(KeychainKind::External)
        .to_string();
    let int_desc = wallet
        .public_descriptor(KeychainKind::Internal)
        .to_string();
    let ext_cs = wallet.descriptor_checksum(KeychainKind::External);
    let int_cs = wallet.descriptor_checksum(KeychainKind::Internal);

    Ok((format!("{}#{}", ext_desc, ext_cs), format!("{}#{}", int_desc, int_cs)))
}

pub fn ensure_core_watchonly(
    base_url: &str,
    rpc_user: &Option<String>,
    rpc_pass: &Option<String>,
    wallet_name: &str,
) -> Result<()> {
    use bitcoincore_rpc::RpcApi;
    use serde_json::json;

    let auth = match (rpc_user, rpc_pass) {
        (Some(user), Some(pass)) => bitcoincore_rpc::Auth::UserPass(user.clone(), pass.clone()),
        _ => bitcoincore_rpc::Auth::None,
    };
    let root = bitcoincore_rpc::Client::new(base_url, auth)
        .context("creating root RPC client")?;

    let _ = root.call::<serde_json::Value>(
        "createwallet",
        &[
            json!(wallet_name),
            json!(true),  // disable_private_keys
            json!(true),  // blank
            json!(""),    // passphrase
            json!(false), // avoid_reuse
            json!(true),  // descriptors
        ],
    );

    Ok(())
}

pub fn import_public_descriptors(
    base_url: &str,
    rpc_user: &Option<String>,
    rpc_pass: &Option<String>,
    wallet_name: &str,
    ext_with_cs: &str,
    int_with_cs: &str,
    gap: usize,
    rescan: bool,
) -> Result<()> {
    use bitcoincore_rpc::RpcApi;
    use serde_json::json;

    let auth = match (rpc_user, rpc_pass) {
        (Some(user), Some(pass)) => bitcoincore_rpc::Auth::UserPass(user.clone(), pass.clone()),
        _ => bitcoincore_rpc::Auth::None,
    };

    let wallet_url = format!("{}/wallet/{}", base_url.trim_end_matches('/'), wallet_name);
    let wallet_rpc = bitcoincore_rpc::Client::new(&wallet_url, auth)
        .context("creating wallet RPC client")?;

    let end = (gap as u32).saturating_sub(1);
    let ts_val = if rescan { json!(0) } else { json!("now") };

    let imports = json!([
        {
            "desc": ext_with_cs,
            "active": true,
            "range": [0, end],
            "timestamp": ts_val,
            "internal": false,
            "label": "brc721-external"
        },
        {
            "desc": int_with_cs,
            "active": true,
            "range": [0, end],
            "timestamp": ts_val,
            "internal": true,
            "label": "brc721-internal"
        }
    ]);

    let _res: serde_json::Value = wallet_rpc
        .call("importdescriptors", &[imports])
        .context("importing public descriptors to Core")?;

    Ok(())
}

pub fn setup_watchonly(
    data_dir: &str,
    network: Network,
    rpc_url: &str,
    rpc_user: &Option<String>,
    rpc_pass: &Option<String>,
    wallet_name: &str,
    gap: usize,
    rescan: bool,
) -> Result<()> {
    let base_url = rpc_url.trim_end_matches('/');
    ensure_core_watchonly(base_url, rpc_user, rpc_pass, wallet_name)
        .context("ensuring Core watch-only wallet")?;

    let (ext_with_cs, int_with_cs) = load_public_descriptors_with_checksum(data_dir, network)
        .context("loading public descriptors")?;

    import_public_descriptors(
        base_url,
        rpc_user,
        rpc_pass,
        wallet_name,
        &ext_with_cs,
        &int_with_cs,
        gap,
        rescan,
    )
    .context("importing public descriptors")?;

    Ok(())
}

pub fn get_core_balance(
    base_url: &str,
    rpc_user: &Option<String>,
    rpc_pass: &Option<String>,
    wallet_name: &str,
) -> Result<Amount> {
    use bitcoincore_rpc::RpcApi;

    let auth = match (rpc_user, rpc_pass) {
        (Some(user), Some(pass)) => bitcoincore_rpc::Auth::UserPass(user.clone(), pass.clone()),
        _ => bitcoincore_rpc::Auth::None,
    };
    let base = base_url.trim_end_matches('/').to_string();
    let wallet_url = format!("{}/wallet/{}", base, wallet_name);
    let rpc = bitcoincore_rpc::Client::new(&wallet_url, auth)
        .context("creating wallet RPC client")?;
    let bal = rpc.get_balance(None, None)?;
    Ok(bal)
}

pub fn derive_next_address(
    data_dir: &str,
    network: Network,
    keychain: KeychainKind,
) -> Result<String> {
    let db_path = wallet_db_path(data_dir, network);
    let mut conn = Connection::open(&db_path)?;

    let mut wallet = LoadParams::new()
        .check_network(network)
        .load_wallet(&mut conn)?
        .ok_or_else(|| anyhow!("wallet not initialized"))?;

    let addr = wallet.reveal_next_address(keychain).address.to_string();
    let _ = wallet.persist(&mut conn)?;
    Ok(addr)
}

pub fn peek_address(
    data_dir: &str,
    network: Network,
    keychain: KeychainKind,
    index: u32,
) -> Result<String> {
    let db_path = wallet_db_path(data_dir, network);
    let mut conn = Connection::open(&db_path)?;

    let wallet = LoadParams::new()
        .check_network(network)
        .load_wallet(&mut conn)?
        .ok_or_else(|| anyhow!("wallet not initialized"))?;

    let addr = wallet.peek_address(keychain, index).to_string();
    Ok(addr)
}
