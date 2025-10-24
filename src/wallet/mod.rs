pub mod paths;
pub mod types;

use crate::wallet::types::CoreRpc;
use anyhow::{anyhow, Context, Result};
use bdk_wallet::{
    keys::bip39::{Language, Mnemonic, WordCount},
    template::Bip86,
    CreateParams, KeychainKind, LoadParams,
};
use bitcoin::{Amount, Network};
use bitcoincore_rpc::{Auth, RpcApi};
use rusqlite::Connection;
use serde_json::json;
use std::path::PathBuf;

use paths::wallet_db_path;

pub struct Wallet {
    data_dir: PathBuf,
    network: Network,
}

impl Wallet {
    pub fn new<P: Into<PathBuf>>(data_dir: P, network: Network) -> Self {
        Self {
            data_dir: data_dir.into(),
            network,
        }
    }

    pub fn init(
        &self,
        mnemonic: Option<String>,
        passphrase: Option<String>,
    ) -> Result<types::InitResult> {
        let db_path = wallet_db_path(self.data_dir_str(), self.network);
        let mut conn = Connection::open(&db_path)?;

        if let Some(_wallet) = LoadParams::new()
            .check_network(self.network)
            .load_wallet(&mut conn)?
        {
            return Ok(types::InitResult {
                created: false,
                mnemonic: None,
                db_path,
            });
        }

        let mnemonic =
            match mnemonic {
                Some(s) => Mnemonic::parse(s)?,
                None => <Mnemonic as bdk_wallet::keys::GeneratableKey<
                    bdk_wallet::miniscript::Tap,
                >>::generate((WordCount::Words12, Language::English))
                .map_err(|e| {
                    e.map(Into::into)
                        .unwrap_or_else(|| anyhow!("failed to generate mnemonic"))
                })?
                .into_key(),
            };

        let ext = Bip86(
            (mnemonic.clone(), passphrase.clone()),
            KeychainKind::External,
        );
        let int = Bip86((mnemonic.clone(), passphrase), KeychainKind::Internal);

        let _wallet = CreateParams::new(ext, int)
            .network(self.network)
            .create_wallet(&mut conn)?;

        Ok(types::InitResult {
            created: true,
            mnemonic: Some(mnemonic),
            db_path,
        })
    }

    pub fn address(&self, keychain: KeychainKind) -> Result<String> {
        let db_path = wallet_db_path(self.data_dir_str(), self.network);
        let mut conn = Connection::open(&db_path)?;

        let wallet = LoadParams::new()
            .check_network(self.network)
            .load_wallet(&mut conn)?
            .ok_or_else(|| anyhow!("wallet not initialized"))?;

        let addr = wallet.peek_address(keychain, 0).to_string();
        Ok(addr)
    }

    pub fn local_db_path(&self) -> PathBuf {
        wallet_db_path(self.data_dir_str(), self.network)
    }

    pub fn list_core_wallets<R: CoreRpc>(&self, rpc: &R) -> Result<Vec<(String, bool, bool)>> {
        let loaded = CoreRpc::list_wallets(rpc)?;
        let mut out = Vec::with_capacity(loaded.len());
        for name in loaded {
            let info = CoreRpc::get_wallet_info(rpc, &name)?;
            let pk_enabled = info
                .get("private_keys_enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let descriptors = info
                .get("descriptors")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let watch_only = !pk_enabled;
            out.push((name, watch_only, descriptors));
        }
        Ok(out)
    }

    pub fn setup_watchonly(
        &self,
        rpc_url: &str,
        auth: &Auth,
        wallet_name: &str,
        rescan: bool,
    ) -> Result<()> {
        let base_url = rpc_url.trim_end_matches('/');
        self.ensure_core_watchonly(base_url, auth, wallet_name)
            .context("ensuring Core watch-only wallet")?;

        let (ext_with_cs, int_with_cs) = self
            .public_descriptors_with_checksum()
            .context("loading public descriptors")?;

        let wallet_url = format!("{}/wallet/{}", base_url.trim_end_matches('/'), wallet_name);
        let wallet_rpc = bitcoincore_rpc::Client::new(&wallet_url, auth.clone())
            .context("creating wallet RPC client")?;

        let end = 0u32;
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

    pub fn core_balance(&self, rpc_url: &str, auth: &Auth, wallet_name: &str) -> Result<Amount> {
        let base = rpc_url.trim_end_matches('/').to_string();
        let rpc = crate::wallet::types::RealCoreRpc::new(base, auth.clone());
        let bal = CoreRpc::get_wallet_balance(&rpc, wallet_name)?;
        Ok(bal)
    }

    fn data_dir_str(&self) -> &str {
        self.data_dir.to_str().unwrap_or("")
    }

    pub fn public_descriptors_with_checksum(&self) -> Result<(String, String)> {
        let db_path = wallet_db_path(self.data_dir_str(), self.network);
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))?;

        let wallet = LoadParams::new()
            .check_network(self.network)
            .load_wallet(&mut conn)?
            .ok_or_else(|| anyhow!("wallet not initialized"))?;

        let ext_desc = wallet.public_descriptor(KeychainKind::External).to_string();
        let int_desc = wallet.public_descriptor(KeychainKind::Internal).to_string();
        let ext_cs = wallet.descriptor_checksum(KeychainKind::External);
        let int_cs = wallet.descriptor_checksum(KeychainKind::Internal);

        Ok((
            format!("{}#{}", ext_desc, ext_cs),
            format!("{}#{}", int_desc, int_cs),
        ))
    }

    pub fn generate_wallet_name(&self) -> Result<String> {
        // Retrieve public descriptors with checksum (external and internal)
        let (ext_with_cs, _int_with_cs) = self
            .public_descriptors_with_checksum()
            .context("loading public descriptors")?;
        // Prepare to create a short, unique wallet name based on hashed descriptor
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;
        hasher.update(ext_with_cs.as_bytes());
        let hash = hasher.finalize();
        // Use first 4 bytes of the hash as a short identifier
        let short = hex::encode(&hash[..4]);
        let wallet_name = format!("brc721-{}-{}", short, self.network);
        Ok(wallet_name)
    }

    fn ensure_core_watchonly(&self, base_url: &str, auth: &Auth, wallet_name: &str) -> Result<()> {
        let root = bitcoincore_rpc::Client::new(base_url, auth.clone())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{distributions::Alphanumeric, Rng};

    fn temp_data_dir() -> PathBuf {
        let mut base = std::env::temp_dir();
        let suffix: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(12)
            .map(char::from)
            .collect();
        base.push(format!("brc721-test-{}", suffix));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn wallet_single_address_is_deterministic() {
        let data_dir = temp_data_dir();
        let net = bitcoin::Network::Regtest;
        let w = Wallet::new(&data_dir, net);

        let mnemonic = Some("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string());
        let res = w.init(mnemonic, None).expect("init ok");
        assert!(res.db_path.exists());

        let ext1 = w.address(KeychainKind::External).expect("ext addr");
        let ext2 = w.address(KeychainKind::External).expect("ext addr again");
        assert_eq!(ext1, ext2, "external address should be stable");

        let int1 = w.address(KeychainKind::Internal).expect("int addr");
        let int2 = w.address(KeychainKind::Internal).expect("int addr again");
        assert_eq!(int1, int2, "internal address should be stable");

        assert_ne!(ext1, int1, "external and internal addresses differ");

        let w2 = Wallet::new(&data_dir, net);
        let ext_again = w2
            .address(KeychainKind::External)
            .expect("ext addr after reload");
        assert_eq!(
            ext1, ext_again,
            "address should be deterministic across instances"
        );
    }
}
