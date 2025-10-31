use std::path::Path;
use url::Url;

use anyhow::{Context, Ok, Result};
use bdk_wallet::{
    bip39::Mnemonic, template::Bip86, AddressInfo, KeychainKind, LoadParams, PersistedWallet,
    Wallet,
};
use bitcoin::{bip32::Xpriv, Network};
use bitcoincore_rpc::{json, Auth, Client, RpcApi};
use rand::{rngs::OsRng, RngCore};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::wallet::paths;

pub struct Brc721Wallet {
    wallet: PersistedWallet<Connection>,
    conn: Connection,
}

impl Brc721Wallet {
    pub fn create<P: AsRef<Path>>(
        data_dir: P,
        network: Network,
        mnemonic: Option<Mnemonic>,
        passphrase: Option<String>,
    ) -> Result<Brc721Wallet> {
        let mnemonic = mnemonic.unwrap_or_else(|| {
            let mut entropy = [0u8; 32];
            OsRng.fill_bytes(&mut entropy);
            let m = Mnemonic::from_entropy(&entropy).expect("mnemonic");
            eprintln!("{m}");
            m
        });

        // Derive BIP32 master private key from seed.
        let seed = mnemonic.to_seed(passphrase.unwrap_or_default());
        let master_xprv = Xpriv::new_master(network, &seed).expect("master_key");
        let external = Bip86(master_xprv, KeychainKind::External);
        let internal = Bip86(master_xprv, KeychainKind::Internal);

        let db_path = paths::wallet_db_path(data_dir, network);
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))?;

        let wallet = Wallet::create(external, internal)
            .network(network)
            .create_wallet(&mut conn)?;

        Ok(Self { wallet, conn })
    }

    pub fn load<P: AsRef<Path>>(data_dir: P, network: Network) -> Result<Brc721Wallet> {
        let db_path = paths::wallet_db_path(data_dir, network);
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))?;
        let wallet = LoadParams::new()
            .check_network(network)
            .load_wallet(&mut conn)
            .context("loading wallet")?;

        wallet
            .map(|wallet| Self { wallet, conn })
            .context("wallet not found")
    }

    pub fn id(&self) -> String {
        let external = self.wallet.public_descriptor(KeychainKind::External);
        let internal = self.wallet.public_descriptor(KeychainKind::Internal);
        let combined = format!("{external}{internal}");
        let hash = Sha256::digest(combined.as_bytes());
        hex::encode(hash)
    }

    pub fn reveal_next_payment_address(&mut self) -> Result<AddressInfo> {
        let address = self.wallet.reveal_next_address(KeychainKind::External);
        self.wallet
            .persist(&mut self.conn)
            .context("persisting the wallet")?;
        Ok(address)
    }

    pub fn balances(&self, rpc_url: &Url, auth: Auth) -> Result<json::GetBalancesResult> {
        let watch_name = self.id();
        let watch_url = format!(
            "{}/wallet/{}",
            rpc_url.to_string().trim_end_matches('/'),
            watch_name
        );
        let watch_client = Client::new(&watch_url, auth).expect("watch client");
        watch_client.get_balances().context("get balance")
    }

    pub fn setup_watch_only(&self, rpc_url: &Url, auth: Auth) -> Result<()> {
        let watch_name = self.id();
        let root_client = Client::new(rpc_url.as_ref(), auth.clone()).unwrap();

        // Check if the watch-only wallet already exists
        let existing_wallets: Vec<String> = root_client.list_wallets().context("list wallets")?;
        if existing_wallets.contains(&watch_name) {
            return Ok(());
        }

        let ans: serde_json::Value = root_client
            .call::<serde_json::Value>(
                "createwallet",
                &[
                    serde_json::json!(watch_name), // Wallet name
                    serde_json::json!(true),       // Disable private keys
                    serde_json::json!(true),       // Blank wallet
                    serde_json::json!(""),         // Passphrase
                    serde_json::json!(false),      // avoid_reuse
                    serde_json::json!(true),       // Descriptors enabled
                ],
            )
            .context("watch wallet created")?;
        if ans["name"].as_str().unwrap() != watch_name {
            return Err(anyhow::anyhow!("Unexpected wallet name: {:?}", ans["name"]));
        }
        if !ans["warning"].is_null() {
            return Err(anyhow::anyhow!(
                "watch_only wallet created with warning: {}",
                ans["warning"]
            ));
        }

        let watch_url = format!(
            "{}/wallet/{}",
            rpc_url.to_string().trim_end_matches('/'),
            watch_name
        );
        let watch_client = Client::new(&watch_url, auth).expect("watch client");

        // Import the wallet's external and internal public descriptors into the watch-only wallet.
        let imports = serde_json::json!([
            {
                "desc": self.wallet.public_descriptor(KeychainKind::External),
                "timestamp": "now",
                "active": true,
                "range": [0,999],
                "internal": false
            },
            {
                "desc": self.wallet.public_descriptor(KeychainKind::Internal),
                "timestamp": "now",
                "active": true,
                "range": [0,999],
                "internal": true
            }
        ]);
        let ans: serde_json::Value = watch_client
            .call::<serde_json::Value>("importdescriptors", &[imports])
            .context("import descriptor")?;

        let arr = ans.as_array().expect("array");

        // Require all imports to be successful. If not, return error with details.
        if !arr.iter().all(|e| e["success"].as_bool() == Some(true)) {
            return Err(anyhow::anyhow!(
                "Failed to import descriptors: {}",
                serde_json::to_string_pretty(&ans).unwrap()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bdk_wallet::bip39::Language;
    use tempfile::TempDir;

    #[test]
    fn test_wallet_id_output_is_as_expected() {
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");
        let data_dir = TempDir::new().expect("temp dir");
        let wallet = Brc721Wallet::create(&data_dir, Network::Regtest, Some(mnemonic), None)
            .expect("wallet");
        let wallet_id = wallet.id();
        // The expected id value was calculated against known descriptors for this mnemonic+network
        // If descriptors change, update this value accordingly.
        let expected_id = "0ca60de20e7da91dc9acf9894f27f264008bbb4b0d35f0de068253977e66e1ff";
        assert_eq!(
            wallet_id, expected_id,
            "Wallet id output does not match expected"
        );
    }

    #[test]
    fn test_payment_address_index_persists_across_reloads() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
                Language::English,
                "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
            ).expect("mnemonic");
        let network = Network::Regtest;

        // Create wallet and reveal a couple of payment addresses
        let mut wallet = Brc721Wallet::create(&data_dir, network, Some(mnemonic.clone()), None)
            .expect("create wallet");
        let addr1 = wallet
            .reveal_next_payment_address()
            .expect("address")
            .address;

        // Reload the wallet from storage
        let mut loaded_wallet =
            Brc721Wallet::load(&data_dir, network).expect("load should not fail");

        let addr2 = loaded_wallet
            .reveal_next_payment_address()
            .expect("address")
            .address;
        // addr3 should differ from addr2: index increment is persisted
        assert_ne!(addr1, addr2, "Reloaded wallet should continue index");
    }

    #[test]
    fn test_reveal_next_payment_address_returns_valid_address() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");
        let network = Network::Regtest;
        let mut wallet =
            Brc721Wallet::create(&data_dir, network, Some(mnemonic), None).expect("wallet");
        let address_info = wallet.reveal_next_payment_address().expect("address");
        // Ensure the address is not empty
        assert!(
            !address_info.address.to_string().is_empty(),
            "Address should not be empty"
        );
    }

    #[test]
    fn test_reveal_next_payment_address_increments_index() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");
        let network = Network::Regtest;
        let mut wallet =
            Brc721Wallet::create(&data_dir, network, Some(mnemonic), None).expect("wallet");
        let address_info_1 = wallet.reveal_next_payment_address().unwrap();
        let address_info_2 = wallet.reveal_next_payment_address().unwrap();
        // Next address should be different (index incremented)
        assert_ne!(
            address_info_1.address, address_info_2.address,
            "Two consecutive revealed addresses should differ"
        );
    }

    #[test]
    fn test_load_returns_error_for_unexistent_wallet() {
        let data_dir = TempDir::new().expect("temp dir");
        // No wallet created
        let result = Brc721Wallet::load(&data_dir, Network::Regtest);
        assert!(
            result.is_err(),
            "Expected an error when loading a wallet that doesn't exist"
        );
    }

    #[test]
    fn test_wallet_id_uniqueness_across_networks() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        let wallet_regtest =
            Brc721Wallet::create(&data_dir, Network::Regtest, Some(mnemonic.clone()), None)
                .expect("regtest");
        let wallet_bitcoin =
            Brc721Wallet::create(&data_dir, Network::Bitcoin, Some(mnemonic), None)
                .expect("bitcoin");
        let id_regtest = wallet_regtest.id();
        let id_bitcoin = wallet_bitcoin.id();
        assert_ne!(
            id_regtest, id_bitcoin,
            "Wallet ids on different networks must be different"
        );
    }

    #[test]
    fn test_wallet_id_stable_with_same_inputs() {
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        let data_dir0 = TempDir::new().expect("temp dir");
        let wallet0 =
            Brc721Wallet::create(&data_dir0, Network::Regtest, Some(mnemonic.clone()), None)
                .expect("wallet0");

        let data_dir1 = TempDir::new().expect("temp dir");
        let wallet1 =
            Brc721Wallet::create(&data_dir1, Network::Regtest, Some(mnemonic.clone()), None)
                .expect("wallet1");

        assert_eq!(
            wallet0.id(),
            wallet1.id(),
            "Wallet id should be stable for same mnemonic and network"
        );
    }

    #[test]
    fn test_wallet_id_stable_with_same_inputs_and_passphrase() {
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        let data_dir0 = TempDir::new().expect("temp dir");
        let wallet0 = Brc721Wallet::create(
            &data_dir0,
            Network::Regtest,
            Some(mnemonic.clone()),
            Some("passphrase1".to_string()),
        )
        .expect("wallet0");

        let data_dir1 = TempDir::new().expect("temp dir");
        let wallet1 = Brc721Wallet::create(
            &data_dir1,
            Network::Regtest,
            Some(mnemonic.clone()),
            Some("passphrase1".to_string()),
        )
        .expect("wallet1");

        let data_dir2 = TempDir::new().expect("temp dir");
        let wallet2 = Brc721Wallet::create(
            &data_dir2,
            Network::Regtest,
            Some(mnemonic.clone()),
            Some("passphrase2".to_string()),
        )
        .expect("wallet2");

        assert_eq!(
            wallet0.id(),
            wallet1.id(),
            "Wallet id should be stable for same mnemonic, network and passphrase"
        );

        assert_ne!(
            wallet0.id(),
            wallet2.id(),
            "Wallet id should be different for different passphrases"
        );
    }

    #[test]
    fn test_load_wallet() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        let network = Network::Regtest;
        Brc721Wallet::create(&data_dir, network, Some(mnemonic), None).expect("wallet");
        let wallet = Brc721Wallet::load(&data_dir, network).expect("wallet");
        assert!(!wallet.id().is_empty());
    }

    #[test]
    fn test_regtest_wallet_persist_on_storage() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        Brc721Wallet::create(&data_dir, Network::Regtest, Some(mnemonic), None).expect("wallet");
        let expected_wallet_path = data_dir.path().join("wallet-regtest.sqlite");
        assert!(expected_wallet_path.exists());
    }

    #[test]
    fn test_bitcoin_wallet_persist_on_storage() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        Brc721Wallet::create(&data_dir, Network::Bitcoin, Some(mnemonic), None).expect("wallet");
        let expected_wallet_path = data_dir.path().join("wallet-mainnet.sqlite");
        assert!(expected_wallet_path.exists());
    }

    #[test]
    fn test_wallet_creation_fails_if_db_exists() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        // First creation should succeed
        Brc721Wallet::create(&data_dir, Network::Regtest, Some(mnemonic.clone()), None)
            .expect("first wallet");
        // Second creation should error because the db is already there
        let result = Brc721Wallet::create(&data_dir, Network::Regtest, Some(mnemonic), None);
        assert!(
            result.is_err(),
            "Expected an error when re-creating the wallet"
        );
    }
}
