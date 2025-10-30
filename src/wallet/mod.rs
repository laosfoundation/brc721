pub mod paths;
pub mod types;

use crate::wallet::types::{CoreAdmin, CoreRpc, CoreWalletInfo, RealCoreAdmin};
use anyhow::{anyhow, Context, Result};
use bdk_wallet::{
    keys::bip39::{Language, Mnemonic, WordCount},
    template::Bip86,
    CreateParams, KeychainKind, LoadParams, PersistedWallet,
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

impl Wallet {
    pub fn new<P: Into<PathBuf>, Q: Into<Url>>(data_dir: P, rpc_url: Q) -> Self {
        Self {
            data_dir: data_dir.into(),
            network: Network::Bitcoin,
            rpc_url: rpc_url.into(),
            wallet: None,
        }
    }

    pub fn with_network(mut self, network: Network) -> Self {
        self.network = network;
        self
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

        match &self.wallet {
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

    fn try_load_wallet(&mut self) -> Result<()> {
        let mut conn = self.open_conn()?;
        self.wallet = LoadParams::new()
            .check_network(self.network)
            .load_wallet(&mut conn)?;
        Ok(())
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

        let (ext_with_cs, int_with_cs) = self
            .public_descriptors_with_checksum()
            .context("loading public descriptors")?;

        let ts_val = if rescan { json!(0) } else { json!("now") };

        let imports = json!([
            {
                "desc": ext_with_cs,
                "active": true,
                "range": [0, range_end],
                "timestamp": ts_val,
                "internal": false,
                "label": "brc721-external"
            },
            {
                "desc": int_with_cs,
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
        let bal = CoreRpc::get_wallet_balance(&rpc, wallet_name)?;
        Ok(bal)
    }

    pub fn generate_wallet_name(&self) -> Result<String> {
        let descriptor = self
            .clone()
            .wallet
            .unwrap()
            .clone()
            .public_descriptor(KeychainKind::External)
            .clone();
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;
        hasher.update(descriptor.to_string().as_bytes());
        let hash = hasher.finalize();
        let short = hex::encode(&hash[..4]);
        let wallet_name = format!("brc721-{}-{}", short, self.network);
        Ok(wallet_name)
    }
}
