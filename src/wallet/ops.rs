use bdk_wallet::{
    keys::bip39::{Language, Mnemonic, WordCount},
    template::Bip86,
    CreateParams, KeychainKind, LoadParams,
};
use bitcoin::Network;
use rusqlite::Connection;

use super::paths::wallet_db_path;
use super::types::InitResult;

pub fn init_wallet(
    data_dir: &str,
    network: Network,
    mnemonic_str: Option<String>,
    passphrase: Option<String>,
) -> Result<InitResult, String> {
    let db_path = wallet_db_path(data_dir, network);
    let mut conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    if let Some(_wallet) = LoadParams::new()
        .check_network(network)
        .load_wallet(&mut conn)
        .map_err(|e| format!("{}", e))?
    {
        return Ok(InitResult {
            created: false,
            mnemonic: None,
            db_path,
        });
    }

    let mnemonic = match mnemonic_str {
        Some(s) => Mnemonic::parse(s).map_err(|e| format!("{}", e))?,
        None => <Mnemonic as bdk_wallet::keys::GeneratableKey<bdk_wallet::miniscript::Tap>>::generate((WordCount::Words12, Language::English))
            .map_err(|e| format!("{:?}", e))?
            .into_key(),
    };

    let ext = Bip86((mnemonic.clone(), passphrase.clone()), KeychainKind::External);
    let int = Bip86((mnemonic.clone(), passphrase), KeychainKind::Internal);

    let _wallet = CreateParams::new(ext, int)
        .network(network)
        .create_wallet(&mut conn)
        .map_err(|e| format!("{}", e))?;

    Ok(InitResult {
        created: true,
        mnemonic: Some(mnemonic),
        db_path,
    })
}

pub fn next_address(
    data_dir: &str,
    network: Network,
) -> Result<String, String> {
    let db_path = wallet_db_path(data_dir, network);
    let mut conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    let mut wallet = LoadParams::new()
        .check_network(network)
        .load_wallet(&mut conn)
        .map_err(|e| format!("{}", e))?
        .ok_or_else(|| "wallet not initialized".to_string())?;

    let addr = wallet.reveal_next_address(KeychainKind::External).address.to_string();
    let _ = wallet.persist(&mut conn).map_err(|e| format!("{}", e))?;
    Ok(addr)
}
