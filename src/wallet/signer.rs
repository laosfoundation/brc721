use age::secrecy::SecretString;
use anyhow::Result;
use bdk_wallet::{template::Bip86, KeychainKind, Wallet};
use bitcoin::{Network, Psbt};

use crate::wallet::master_key_store::MasterKeyStore;

/// Signer provides a builder-style API (with_*) to configure
/// and produce signatures for PSBTs using the wallet's master key material.
pub struct Signer {
    data_dir: std::path::PathBuf,
    network: Network,
}

impl Signer {
    pub fn new() -> Self {
        Self {
            data_dir: std::path::PathBuf::new(),
            network: Network::Regtest,
        }
    }

    pub fn with_data_dir<P: AsRef<std::path::Path>>(mut self, data_dir: P) -> Self {
        self.data_dir = data_dir.as_ref().to_path_buf();
        self
    }

    pub fn with_network(mut self, network: Network) -> Self {
        self.network = network;
        self
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
