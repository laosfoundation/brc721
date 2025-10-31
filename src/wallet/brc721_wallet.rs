use std::path::Path;

use anyhow::{Context, Result};
use bdk_wallet::{
    bip39::{Language, Mnemonic},
    template::Bip86,
    CreateParams, KeychainKind, LoadParams, PersistedWallet, Wallet,
};
use bitcoin::{bip32::Xpriv, hashes::sha256, network, Network};
use rusqlite::Connection;

use crate::wallet::paths;
struct Brc721Wallet {
    wallet: PersistedWallet<Connection>,
}

impl Brc721Wallet {
    fn create<P: AsRef<Path>>(
        data_dir: P,
        network: Network,
        mnemonic: Mnemonic,
    ) -> Result<Brc721Wallet> {
        // Derive BIP32 master private key from seed.
        let seed = mnemonic.to_seed(String::new()); // empty password
        let master_xprv = Xpriv::new_master(network, &seed).expect("master_key");
        let external = Bip86(master_xprv, KeychainKind::External);
        let internal = Bip86(master_xprv, KeychainKind::Internal);

        let db_path = paths::wallet_db_path(data_dir, network);
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))?;

        let wallet = Wallet::create(external, internal)
            .network(network)
            .create_wallet(&mut conn)?;

        Ok(Self { wallet })
    }

    fn load<P: AsRef<Path>>(data_dir: P, network: Network) -> Result<Brc721Wallet> {
        let db_path = paths::wallet_db_path(data_dir, network);
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))?;
        let wallet = LoadParams::new()
            .check_network(network)
            .load_wallet(&mut conn)
            .context("loading wallet")?
            .unwrap();
        Ok(Self { wallet })
    }

    fn id(&self) -> String {
        let external = self.wallet.public_descriptor(KeychainKind::External);
        let internal = self.wallet.public_descriptor(KeychainKind::Internal);
        let combined = format!("{external}{internal}");
        let digest = sha256::Hash::const_hash(combined.as_bytes());
        digest.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_fails_for_unexistent_wallet() {
        let data_dir = TempDir::new().expect("temp dir");
        // No wallet created
        // Temporarily change the working directory or inject a proper path, if needed
        let result = Brc721Wallet::load(data_dir, Network::Regtest);
        assert!(
            result.is_err(),
            "Expected error when loading a wallet that doesn't exist"
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
            Brc721Wallet::create(&data_dir, Network::Regtest, mnemonic.clone()).expect("regtest");
        let wallet_bitcoin =
            Brc721Wallet::create(&data_dir, Network::Bitcoin, mnemonic).expect("bitcoin");
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
            Brc721Wallet::create(&data_dir0, Network::Regtest, mnemonic.clone()).expect("wallet0");

        let data_dir1 = TempDir::new().expect("temp dir");
        let wallet1 =
            Brc721Wallet::create(&data_dir1, Network::Regtest, mnemonic.clone()).expect("wallet1");

        assert_eq!(
            wallet0.id(),
            wallet1.id(),
            "Wallet id should be stable for same mnemonic and network"
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
        Brc721Wallet::create(&data_dir, network, mnemonic).expect("wallet");
        Brc721Wallet::load(&data_dir, network).expect("wallet");
    }

    #[test]
    fn test_regtest_wallet_persist_on_storage() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        Brc721Wallet::create(&data_dir, Network::Regtest, mnemonic).expect("wallet");
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

        Brc721Wallet::create(&data_dir, Network::Bitcoin, mnemonic).expect("wallet");
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
        Brc721Wallet::create(&data_dir, Network::Regtest, mnemonic.clone()).expect("first wallet");
        // Second creation should error because the db is already there
        let result = Brc721Wallet::create(&data_dir, Network::Regtest, mnemonic);
        assert!(
            result.is_err(),
            "Expected an error when re-creating the wallet"
        );
    }
}
