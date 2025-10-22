use bdk_wallet::{
    keys::bip39::{Language, Mnemonic, WordCount},
    template::Bip86,
    CreateParams, KeychainKind, LoadParams,
};
use bitcoin::Network;
use rusqlite::Connection;
use anyhow::{Result, anyhow};

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
        None => <Mnemonic as bdk_wallet::keys::GeneratableKey<bdk_wallet::miniscript::Tap>>::generate((WordCount::Words12, Language::English))
            .map_err(|e| e.map(Into::into).unwrap_or_else(|| anyhow!("failed to generate mnemonic")))?
            .into_key(),
    };

    let ext = Bip86((mnemonic.clone(), passphrase.clone()), KeychainKind::External);
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

pub fn next_address(
    data_dir: &str,
    network: Network,
) -> Result<String> {
    let db_path = wallet_db_path(data_dir, network);
    let mut conn = Connection::open(&db_path)?;

    let mut wallet = LoadParams::new()
        .check_network(network)
        .load_wallet(&mut conn)?
        .ok_or_else(|| anyhow!("wallet not initialized"))?;

    let addr = wallet.reveal_next_address(KeychainKind::External).address.to_string();
    let _ = wallet.persist(&mut conn)?;
    Ok(addr)
}
