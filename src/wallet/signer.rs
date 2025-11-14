use crate::wallet::master_key_store::MasterKeyStore;
use age::secrecy::SecretString;
use anyhow::Result;
use bdk_wallet::{template::Bip86, KeychainKind, Wallet};
use bitcoin::bip32::Xpriv;
use bitcoin::{Network, Psbt};
use std::path::Path;

/// Signer provides a builder-style API (with_*) to configure
/// and produce signatures for PSBTs using the wallet's master key material.
pub struct Signer {
    data_dir: std::path::PathBuf,
    network: Network,
}

impl Signer {
    pub fn new<P: AsRef<Path>>(data_dir: P, network: Network) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
            network,
        }
    }

    /// Persist the provided master private key using MasterKeyStore with encryption.
    pub fn store_master_key(&self, xpriv: &Xpriv, passphrase: &SecretString) -> Result<()> {
        let store = MasterKeyStore::new(&self.data_dir);
        store.store(xpriv, passphrase)
    }

    /// Sign the provided PSBT using the wallet's master private key stored in MasterKeyStore.
    /// The passphrase is provided at call time and is not stored by the Signer.
    pub fn sign(&self, psbt: &mut Psbt, passphrase: &SecretString) -> Result<bool> {
        let store = MasterKeyStore::new(&self.data_dir);
        let master_xprv = store.load(passphrase)?;
        let external = Bip86(master_xprv, KeychainKind::External);
        let internal = Bip86(master_xprv, KeychainKind::Internal);

        let wallet = Wallet::create(external, internal)
            .network(self.network)
            .create_wallet_no_persist()?;

        let finalized = wallet.sign(psbt, Default::default()).expect("sign");
        Ok(finalized)
    }
}
