pub mod paths;
pub mod types;

use crate::wallet::types::{CoreAdmin, CoreRpc, CoreWalletInfo, RealCoreAdmin};
use anyhow::{anyhow, Context, Result};
use bdk_wallet::{
    keys::bip39::{Language, Mnemonic, WordCount},
    template::Bip86,
    CreateParams, KeychainKind, LoadParams,
};
use bitcoin::{Address, Amount, Network};
use bitcoincore_rpc::Auth;
use rusqlite::Connection;
use serde_json::json;
use std::path::PathBuf;
use url::Url;

use paths::wallet_db_path;

#[derive(Debug)]
pub struct Wallet {
    data_dir: PathBuf,
    network: Network,
    rpc_url: Url,
    wallet: Option<bdk_wallet::PersistedWallet<Connection>>,
}

pub struct WalletBuilder {
    data_dir: PathBuf,
    network: Network,
    rpc_url: Url,
}

impl WalletBuilder {
    pub fn new<P: Into<PathBuf>, Q: Into<Url>>(data_dir: P, rpc_url: Q) -> Self {
        Self {
            data_dir: data_dir.into(),
            network: Network::Bitcoin,
            rpc_url: rpc_url.into(),
        }
    }

    pub fn with_network(mut self, network: Network) -> Self {
        self.network = network;
        self
    }

    pub fn build(self) -> Wallet {
        Wallet {
            data_dir: self.data_dir,
            network: self.network,
            rpc_url: self.rpc_url,
            wallet: None,
        }
    }
}

impl Wallet {
    pub fn builder<P: Into<PathBuf>, Q: Into<Url>>(data_dir: P, rpc_url: Q) -> WalletBuilder {
        WalletBuilder::new(data_dir, rpc_url)
    }

    pub fn init(
        &mut self,
        mnemonic: Option<String>,
        passphrase: Option<String>,
    ) -> Result<types::InitResult> {
        let db_path = self.local_db_path();
        let mut conn = self.open_conn()?;

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

        self.wallet = Some(
            CreateParams::new(ext, int)
                .network(self.network)
                .create_wallet(&mut conn)?,
        );

        Ok(types::InitResult {
            created: true,
            mnemonic: Some(mnemonic),
            db_path,
        })
    }

    pub fn address(&mut self, keychain: KeychainKind) -> Result<Address> {
        if self.wallet.is_none() {
            return Err(anyhow!("No wallet initialized"));
        }

        match &mut self.wallet {
            Some(wallet) => Ok(wallet.reveal_next_address(keychain).address),
            None => Err(anyhow!("No wallet initialized")),
        }
    }

    pub fn local_db_path(&self) -> PathBuf {
        wallet_db_path(&self.data_dir, self.network)
    }

    fn open_conn(&self) -> Result<Connection> {
        let db_path = self.local_db_path();
        Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))
    }

    pub fn public_descriptors_with_checksum(&self) -> Result<(String, String)> {
        let mut conn = self.open_conn()?;
        let wallet = LoadParams::new()
            .check_network(self.network)
            .load_wallet(&mut conn)?
            .ok_or_else(|| anyhow!("wallet not initialized"))?;
        let ext = wallet.public_descriptor(KeychainKind::External);
        let int = wallet.public_descriptor(KeychainKind::Internal);
        Ok((ext.to_string(), int.to_string()))
    }

    pub fn list_core_wallets<R: CoreRpc>(&self, rpc: &R) -> Result<Vec<CoreWalletInfo>> {
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
            out.push(CoreWalletInfo {
                name,
                watch_only,
                descriptors,
            });
        }
        Ok(out)
    }

    pub fn setup_watchonly(&self, auth: &Auth, wallet_name: &str, rescan: bool) -> Result<()> {
        let base_url = self.rpc_url.to_string();
        let admin = RealCoreAdmin::new(base_url, auth.clone());
        self.setup_watchonly_with(&admin, wallet_name, 1000, rescan)
    }

    pub fn setup_watchonly_with<A: CoreAdmin>(
        &self,
        admin: &A,
        wallet_name: &str,
        range_end: u32,
        rescan: bool,
    ) -> Result<()> {
        admin
            .ensure_watchonly_descriptor_wallet(wallet_name)
            .context("ensuring Core watch-only wallet")?;

        let (ext, int) = self
            .public_descriptors_with_checksum()
            .context("loading public descriptors")?;

        let ts_val = if rescan { json!(0) } else { json!("now") };

        let imports = json!([
            {
                "desc": ext,
                "active": true,
                "range": [0, range_end],
                "timestamp": ts_val,
                "internal": false,
                "label": "brc721-external"
            },
            {
                "desc": int,
                "active": true,
                "range": [0, range_end],
                "timestamp": ts_val,
                "internal": true,
                "label": "brc721-internal"
            }
        ]);

        admin
            .import_descriptors(wallet_name, imports)
            .context("importing public descriptors to Core")?;

        Ok(())
    }

    pub fn core_balance(&self, auth: &Auth, wallet_name: &str) -> Result<Amount> {
        let base = self.rpc_url.to_string();
        let rpc = crate::wallet::types::RealCoreRpc::new(base, auth.clone());
        // Core may need a moment after import; wait for non-zero balance
        for _ in 0..50 {
            if let Ok(bal) = CoreRpc::get_wallet_balance(&rpc, wallet_name) {
                if bal.to_sat() > 0 {
                    return Ok(bal);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let bal = CoreRpc::get_wallet_balance(&rpc, wallet_name)?;
        Ok(bal)
    }

    fn generate_wallet_name(&self) -> Result<String> {
        let mut conn = self.open_conn()?;
        let wallet = LoadParams::new()
            .check_network(self.network)
            .load_wallet(&mut conn)?
            .ok_or_else(|| anyhow!("wallet not initialized"))?;
        let descriptor = wallet.public_descriptor(KeychainKind::External);
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;
        hasher.update(descriptor.to_string().as_bytes());
        let hash = hasher.finalize();
        let short = hex::encode(&hash[..4]);
        let wallet_name = format!("brc721-{}-{}", short, self.network);
        Ok(wallet_name)
    }

    pub fn name(&self) -> Result<String> {
        self.generate_wallet_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use url::Url;

    #[test]
    fn test_name() {
        // Setup a temp directory for the wallet data
        let data_dir = TempDir::new().expect("create temp dir");
        let rpc_url = Url::parse("http://localhost:18332").expect("valid url");
        let mut wallet = Wallet::builder(data_dir.path(), rpc_url)
            .with_network(bitcoin::Network::Regtest)
            .build();
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

        let init_res = wallet.init(Some(mnemonic.to_string()), None);
        assert!(init_res.is_ok());
        let name = wallet.name().expect("wallet name");
        assert_eq!(name, "brc721-0bf90487-regtest");
        let name2 = wallet.name().expect("Second call to name");
        assert_eq!(name, name2);
    }
}
