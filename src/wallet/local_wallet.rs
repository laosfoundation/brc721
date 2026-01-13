use anyhow::{Context, Result};
use bdk_wallet::{template::Bip86, AddressInfo, KeychainKind, LoadParams, PersistedWallet, Wallet};
use bitcoin::{bip32::Xpriv, Network};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn wallet_db_path<P: AsRef<Path>>(data_dir: P) -> PathBuf {
    let mut p = PathBuf::from(data_dir.as_ref());
    p.push("wallet.sqlite");
    p
}

pub struct LocalWallet {
    wallet: PersistedWallet<Connection>,
    conn: Connection,
}

impl LocalWallet {
    pub fn create<P: AsRef<Path>>(
        data_dir: P,
        network: Network,
        external: Bip86<Xpriv>,
        internal: Bip86<Xpriv>,
    ) -> Result<LocalWallet> {
        if !data_dir.as_ref().exists() {
            std::fs::create_dir_all(&data_dir).context("creating wallet directory")?;
        }
        let db_path = wallet_db_path(&data_dir);
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("opening wallet db at {}", db_path.display()))?;

        let wallet = Wallet::create(external, internal)
            .network(network)
            .create_wallet(&mut conn)?;

        Ok(Self { wallet, conn })
    }

    pub fn load<P: AsRef<Path>>(data_dir: P, network: Network) -> Result<LocalWallet> {
        let db_path = wallet_db_path(&data_dir);
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

    pub fn revealed_payment_addresses(&self) -> Vec<AddressInfo> {
        let Some(last_revealed_index) = self.wallet.derivation_index(KeychainKind::External) else {
            return Vec::new();
        };

        (0..=last_revealed_index)
            .map(|index| self.wallet.peek_address(KeychainKind::External, index))
            .collect()
    }

    pub fn public_descriptor(&self, keychain: KeychainKind) -> String {
        self.wallet.public_descriptor(keychain).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bdk_wallet::bip39::{Language, Mnemonic};
    use tempfile::TempDir;

    #[test]
    fn revealed_payment_addresses_lists_revealed_only() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        )
        .expect("mnemonic");

        let seed = mnemonic.to_seed(String::default());
        let master_xprv = Xpriv::new_master(Network::Regtest, &seed).expect("master key");
        let external = Bip86(master_xprv, KeychainKind::External);
        let internal = Bip86(master_xprv, KeychainKind::Internal);

        let mut wallet = LocalWallet::create(data_dir.path(), Network::Regtest, external, internal)
            .expect("create wallet");

        assert!(wallet.revealed_payment_addresses().is_empty());

        let addr0 = wallet
            .reveal_next_payment_address()
            .expect("address 0")
            .address
            .to_string();
        let addr1 = wallet
            .reveal_next_payment_address()
            .expect("address 1")
            .address
            .to_string();

        drop(wallet);

        let loaded = LocalWallet::load(data_dir.path(), Network::Regtest).expect("load wallet");
        let revealed = loaded.revealed_payment_addresses();
        assert_eq!(revealed.len(), 2);
        assert_eq!(revealed[0].index, 0);
        assert_eq!(revealed[0].address.to_string(), addr0);
        assert_eq!(revealed[1].index, 1);
        assert_eq!(revealed[1].address.to_string(), addr1);
    }
}
