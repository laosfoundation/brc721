use std::path::Path;

use anyhow::{Context, Result};
use bdk_wallet::{
    bip39::{Language, Mnemonic},
    template::Bip86,
    CreateParams, KeychainKind,
};
use bitcoin::{bip32::Xpriv, Network};
use rusqlite::Connection;

use crate::wallet::paths;
struct Brc721Wallet;

impl Brc721Wallet {
    fn get_or_create<P: AsRef<Path>>(data_dir: P, network: Network) -> Result<Brc721Wallet> {
        // Parse the deterministic 12-word BIP39 mnemonic seed phrase.
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).unwrap();

        // Derive BIP32 master private key from seed.
        let seed = mnemonic.to_seed(String::new()); // empty password
        let master_xprv = Xpriv::new_master(network, &seed).expect("master_key");
        let external = Bip86(master_xprv, KeychainKind::External);
        let internal = Bip86(master_xprv, KeychainKind::Internal);

        let db_path = paths::wallet_db_path(data_dir, network);
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))?;

        CreateParams::new(external, internal)
            .network(network)
            .create_wallet(&mut conn)?;

        Ok(Self {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_regtest_wallet_persist_on_storage() {
        let data_dir = TempDir::new().expect("temp dir");
        Brc721Wallet::get_or_create(&data_dir, Network::Regtest).expect("wallet");
        let expected_wallet_path = data_dir.path().join("wallet-regtest.sqlite");
        assert!(expected_wallet_path.exists());
    }

    #[test]
    fn test_bitcoin_wallet_persist_on_storage() {
        let data_dir = TempDir::new().expect("temp dir");
        Brc721Wallet::get_or_create(&data_dir, Network::Bitcoin).expect("wallet");
        let expected_wallet_path = data_dir.path().join("wallet-mainnet.sqlite");
        assert!(expected_wallet_path.exists());
    }
}
