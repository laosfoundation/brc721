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
        master_xprv: &Xpriv,
    ) -> Result<LocalWallet> {
        let external = Bip86(*master_xprv, KeychainKind::External);
        let internal = Bip86(*master_xprv, KeychainKind::Internal);

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

    pub fn public_descriptor(&self, keychain: KeychainKind) -> String {
        self.wallet.public_descriptor(keychain).to_string()
    }
}
