pub mod brc721_wallet;
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

use paths::wallet_db_path;

#[derive(Debug)]
pub struct Wallet {
    data_dir: PathBuf,
    network: Network,
    permanent_wallet: Option<PersistedWallet<Connection>>,
}

impl Wallet {
    pub fn new<P: Into<PathBuf>>(data_dir: P, network: Network) -> Self {
        Self {
            data_dir: data_dir.into(),
            network,
            permanent_wallet: None,
        }
    }

    pub fn load_or_create<P: Into<PathBuf> + Clone>(
        &self,
        data_dir: P,
        network: Network,
        mnemonic: Option<String>,
        passphrase: Option<String>,
    ) -> Result<Wallet> {
        let db_path = wallet_db_path(data_dir.clone().into(), network);
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))?;

        if let Some(wallet) = LoadParams::new()
            .check_network(network)
            .load_wallet(&mut conn)?
        {
            return Ok(Self {
                data_dir: data_dir.into(),
                network,
                permanent_wallet: Some(wallet),
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

        let wallet = Some(
            CreateParams::new(ext, int)
                .network(network)
                .create_wallet(&mut conn)?,
        );

        Ok(Self {
            data_dir: data_dir.into(),
            network,
            permanent_wallet: wallet,
        })
    }

    pub fn init(
        &self,
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

        let _wallet = CreateParams::new(ext, int)
            .network(self.network)
            .create_wallet(&mut conn)?;

        Ok(types::InitResult {
            created: true,
            mnemonic: Some(mnemonic),
            db_path,
        })
    }

    pub fn address(&self, keychain: KeychainKind) -> Result<Address> {
        let mut wallet = self.load_wallet_or_err()?;
        let addr = wallet.reveal_next_address(keychain).address;
        let mut conn = self.open_conn()?;
        wallet.persist(&mut conn)?;
        Ok(addr)
    }

    pub fn local_db_path(&self) -> PathBuf {
        wallet_db_path(&self.data_dir, self.network)
    }

    fn open_conn(&self) -> Result<Connection> {
        let db_path = self.local_db_path();
        Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))
    }

    fn try_load_wallet(&self) -> Result<Option<PersistedWallet<Connection>>> {
        let mut conn = self.open_conn()?;
        let wallet = LoadParams::new()
            .check_network(self.network)
            .load_wallet(&mut conn)?;
        Ok(wallet)
    }

    fn load_wallet_or_err(&self) -> Result<PersistedWallet<Connection>> {
        self.try_load_wallet()?
            .ok_or_else(|| anyhow!("wallet not initialized"))
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

    pub fn setup_watchonly(
        &self,
        rpc_url: &str,
        auth: &Auth,
        wallet_name: &str,
        rescan: bool,
    ) -> Result<()> {
        let base_url = rpc_url.trim_end_matches('/').to_string();
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

    pub fn core_balance(&self, rpc_url: &str, auth: &Auth, wallet_name: &str) -> Result<Amount> {
        let base = rpc_url.trim_end_matches('/').to_string();
        let rpc = crate::wallet::types::RealCoreRpc::new(base, auth.clone());
        let bal = CoreRpc::get_wallet_balance(&rpc, wallet_name)?;
        Ok(bal)
    }

    pub fn public_descriptors_with_checksum(&self) -> Result<(String, String)> {
        let wallet = self.load_wallet_or_err()?;

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
        let (ext_with_cs, _int_with_cs) = self
            .public_descriptors_with_checksum()
            .context("loading public descriptors")?;
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;
        hasher.update(ext_with_cs.as_bytes());
        let hash = hasher.finalize();
        let short = hex::encode(&hash[..4]);
        let wallet_name = format!("brc721-{}-{}", short, self.network);
        Ok(wallet_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{distributions::Alphanumeric, Rng};
    use serde_json::json;
    use std::sync::Mutex;

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
        assert_ne!(ext1, ext2, "external address should advance");

        let int1 = w.address(KeychainKind::Internal).expect("int addr");
        let int2 = w.address(KeychainKind::Internal).expect("int addr again");
        assert_ne!(int1, int2, "internal address should advance");

        assert_ne!(ext1, int1, "external and internal addresses differ");

        let _ext3 = w.address(KeychainKind::External).expect("third ext addr");
        let w2 = Wallet::new(&data_dir, net);
        let ext_again = w2
            .address(KeychainKind::External)
            .expect("ext addr after reload");
        assert_ne!(
            ext2, ext_again,
            "derivation state should advance across instances"
        );
    }

    #[test]
    fn passphrase_affects_derived_addresses() {
        let net = bitcoin::Network::Regtest;
        let mnemonic = Some("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string());

        let dir1 = temp_data_dir();
        let w1 = Wallet::new(&dir1, net);
        w1.init(mnemonic.clone(), Some("pass1".to_string()))
            .unwrap();
        let a1 = w1.address(KeychainKind::External).unwrap();

        let dir2 = temp_data_dir();
        let w2 = Wallet::new(&dir2, net);
        w2.init(mnemonic.clone(), Some("pass2".to_string()))
            .unwrap();
        let a2 = w2.address(KeychainKind::External).unwrap();

        assert_ne!(
            a1, a2,
            "different passphrases should yield different descriptors/addresses"
        );
    }

    #[test]
    fn generate_wallet_name_is_stable_and_unique() {
        let net = bitcoin::Network::Regtest;
        let mnemonic1 = Some("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string());
        let mnemonic2 = Some(
            "legal winner thank year wave sausage worth useful legal winner thank yellow"
                .to_string(),
        );

        let dir1 = temp_data_dir();
        let w1 = Wallet::new(&dir1, net);
        w1.init(mnemonic1, None).unwrap();
        let n1a = w1.generate_wallet_name().unwrap();
        let n1b = w1.generate_wallet_name().unwrap();
        assert_eq!(n1a, n1b, "name should be deterministic");

        let dir2 = temp_data_dir();
        let w2 = Wallet::new(&dir2, net);
        w2.init(mnemonic2, None).unwrap();
        let n2 = w2.generate_wallet_name().unwrap();

        assert_ne!(
            n1a, n2,
            "different descriptors should produce different names"
        );
    }

    #[test]
    fn public_descriptors_error_if_uninitialized() {
        let dir = temp_data_dir();
        let w = Wallet::new(&dir, bitcoin::Network::Regtest);
        let res = w.public_descriptors_with_checksum();
        assert!(res.is_err(), "should error when wallet not initialized");
    }

    struct MockRpc {
        wallets: Vec<String>,
        infos: std::collections::HashMap<String, serde_json::Value>,
    }

    impl CoreRpc for MockRpc {
        fn list_wallets(&self) -> anyhow::Result<Vec<String>> {
            Ok(self.wallets.clone())
        }
        fn get_wallet_info(&self, name: &str) -> anyhow::Result<serde_json::Value> {
            Ok(self.infos.get(name).cloned().unwrap_or_else(|| json!({})))
        }
        fn get_wallet_balance(&self, _name: &str) -> anyhow::Result<Amount> {
            Ok(Amount::from_sat(0))
        }
    }

    #[test]
    fn list_core_wallets_interprets_flags() {
        let dir = temp_data_dir();
        let w = Wallet::new(&dir, bitcoin::Network::Regtest);
        let mut infos = std::collections::HashMap::new();
        infos.insert(
            "wo-desc".to_string(),
            json!({"private_keys_enabled": false, "descriptors": true}),
        );
        infos.insert(
            "legacy".to_string(),
            json!({"private_keys_enabled": true, "descriptors": false}),
        );
        let rpc = MockRpc {
            wallets: vec!["wo-desc".into(), "legacy".into()],
            infos,
        };
        let listed = w.list_core_wallets(&rpc).unwrap();
        assert_eq!(listed.len(), 2);
        let a = &listed[0];
        assert_eq!(a.name, "wo-desc");
        assert!(a.watch_only);
        assert!(a.descriptors);
        let b = &listed[1];
        assert_eq!(b.name, "legacy");
        assert!(!b.watch_only);
        assert!(!b.descriptors);
    }

    struct MockAdmin {
        pub ensured: Mutex<Vec<String>>,
        pub imports: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl CoreAdmin for MockAdmin {
        fn ensure_watchonly_descriptor_wallet(&self, wallet_name: &str) -> anyhow::Result<()> {
            self.ensured.lock().unwrap().push(wallet_name.to_string());
            Ok(())
        }
        fn import_descriptors(
            &self,
            wallet_name: &str,
            imports: serde_json::Value,
        ) -> anyhow::Result<()> {
            self.imports
                .lock()
                .unwrap()
                .push((wallet_name.to_string(), imports));
            Ok(())
        }
    }

    #[test]
    fn setup_watchonly_builds_imports_correctly() {
        let dir = temp_data_dir();
        let net = bitcoin::Network::Regtest;
        let w = Wallet::new(&dir, net);
        let mnemonic = Some("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string());
        w.init(mnemonic, None).unwrap();

        let admin = MockAdmin {
            ensured: Mutex::new(vec![]),
            imports: Mutex::new(vec![]),
        };

        let name = w.generate_wallet_name().unwrap();
        w.setup_watchonly_with(&admin, &name, 5, false).unwrap();

        let ensured = admin.ensured.lock().unwrap();
        assert_eq!(ensured.as_slice(), std::slice::from_ref(&name));
        drop(ensured);

        let imports = admin.imports.lock().unwrap();
        assert_eq!(imports.len(), 1);
        let (n, v) = &imports[0];
        assert_eq!(n, &name);
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        for entry in arr {
            assert_eq!(entry.get("active").unwrap(), &json!(true));
            assert_eq!(entry.get("range").unwrap(), &json!([0, 5]));
            assert!(entry.get("desc").unwrap().as_str().unwrap().contains("#"));
        }
        let ext = &arr[0];
        assert_eq!(ext.get("internal").unwrap(), &json!(false));
        assert_eq!(ext.get("label").unwrap(), &json!("brc721-external"));
        let int = &arr[1];
        assert_eq!(int.get("internal").unwrap(), &json!(true));
        assert_eq!(int.get("label").unwrap(), &json!("brc721-internal"));
        assert_eq!(int.get("timestamp").unwrap(), &json!("now"));
    }
}
