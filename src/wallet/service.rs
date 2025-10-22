use anyhow::{anyhow, Result};
use bdk_wallet::{
    keys::bip39::{Language, Mnemonic, WordCount},
    template::Bip86,
    Balance, CreateParams, KeychainKind, LoadParams,
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

pub fn wallet_balance(data_dir: &str, network: Network) -> Result<Balance> {
    let db_path = wallet_db_path(data_dir, network);
    let mut conn = Connection::open(&db_path)?;

    let wallet = LoadParams::new()
        .check_network(network)
        .load_wallet(&mut conn)?
        .ok_or_else(|| anyhow!("wallet not initialized"))?;

    Ok(wallet.balance())
}

pub fn build_core_createwallet_params(name: &str) -> Vec<serde_json::Value> {
    serde_json::vec![
        serde_json::json!(name),
        serde_json::json!(true),
        serde_json::json!(true),
        serde_json::json!(""),
        serde_json::json!(false),
        serde_json::json!(true)
    ]
}

pub fn build_core_importdescriptors_payload(
    ext_desc_with_cs: &str,
    int_desc_with_cs: &str,
    end: u32,
    rescan: bool,
) -> serde_json::Value {
    let ts = if rescan {
        serde_json::json!(0)
    } else {
        serde_json::json!("now")
    };
    serde_json::json!([
        {
            "desc": ext_desc_with_cs,
            "active": true,
            "range": [0, end],
            "timestamp": ts,
            "internal": false,
            "label": "brc721-external"
        },
        {
            "desc": int_desc_with_cs,
            "active": true,
            "range": [0, end],
            "timestamp": ts,
            "internal": true,
            "label": "brc721-internal"
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_core_createwallet_params() {
        let p = build_core_createwallet_params("wo1");
        assert_eq!(p.len(), 6);
        assert_eq!(p[0], serde_json::json!("wo1"));
        assert_eq!(p[1], serde_json::json!(true));
        assert_eq!(p[2], serde_json::json!(true));
        assert_eq!(p[3], serde_json::json!(""));
        assert_eq!(p[4], serde_json::json!(false));
        assert_eq!(p[5], serde_json::json!(true));
    }

    #[test]
    fn test_build_core_importdescriptors_payload_rescan_true() {
        let v = build_core_importdescriptors_payload("desc1#abcd", "desc2#ef01", 199, true);
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let a0 = arr[0].as_object().unwrap();
        assert_eq!(a0.get("desc").unwrap(), &serde_json::json!("desc1#abcd"));
        assert_eq!(a0.get("active").unwrap(), &serde_json::json!(true));
        assert_eq!(a0.get("range").unwrap(), &serde_json::json!([0, 199]));
        assert_eq!(a0.get("timestamp").unwrap(), &serde_json::json!(0));
        assert_eq!(a0.get("internal").unwrap(), &serde_json::json!(false));
        assert_eq!(a0.get("label").unwrap(), &serde_json::json!("brc721-external"));
        let a1 = arr[1].as_object().unwrap();
        assert_eq!(a1.get("desc").unwrap(), &serde_json::json!("desc2#ef01"));
        assert_eq!(a1.get("internal").unwrap(), &serde_json::json!(true));
        assert_eq!(a1.get("timestamp").unwrap(), &serde_json::json!(0));
    }

    #[test]
    fn test_build_core_importdescriptors_payload_rescan_false() {
        let v = build_core_importdescriptors_payload("desc1#zz", "desc2#yy", 10, false);
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let a0 = arr[0].as_object().unwrap();
        assert_eq!(a0.get("timestamp").unwrap(), &serde_json::json!("now"));
        let a1 = arr[1].as_object().unwrap();
        assert_eq!(a1.get("range").unwrap(), &serde_json::json!([0, 10]));
    }
}
