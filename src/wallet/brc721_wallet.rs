use bitcoincore_rpc::Auth;
use std::path::Path;
use url::Url;

use anyhow::{Context, Ok, Result};
use bdk_wallet::{bip39::Mnemonic, miniscript::psbt::PsbtExt, AddressInfo, KeychainKind};
use bitcoin::{bip32::Xpriv, Address, Amount, Network, Psbt};
use bitcoincore_rpc::json;
use rand::{rngs::OsRng, RngCore};

use crate::wallet::{local_wallet::LocalWallet, remote_wallet::RemoteWallet, signer::Signer};

pub struct Brc721Wallet {
    local: LocalWallet,
    remote: RemoteWallet,
    signer: Signer,
}

impl Brc721Wallet {
    pub fn create<P: AsRef<Path>>(
        data_dir: P,
        network: Network,
        mnemonic: Option<Mnemonic>,
        passphrase: String,
        rpc_url: &Url,
        auth: Auth,
    ) -> Result<Brc721Wallet> {
        let mnemonic = mnemonic.unwrap_or_else(|| {
            let mut entropy = [0u8; 32];
            OsRng.fill_bytes(&mut entropy);
            let m = Mnemonic::from_entropy(&entropy).expect("mnemonic");
            eprintln!("{m}");
            m
        });

        let seed = mnemonic.to_seed(String::default());
        let master_xprv = Xpriv::new_master(network, &seed).expect("master_key");

        let local = LocalWallet::create(&data_dir, network, &master_xprv)?;
        let remote = RemoteWallet::new(local.id(), rpc_url, auth);

        let pass = age::secrecy::SecretString::from(passphrase);
        let signer = Signer::new().with_data_dir(&data_dir).with_network(network);
        signer.store_master_key(&master_xprv, &pass)?;

        Ok(Self {
            local,
            remote,
            signer,
        })
    }

    pub fn load<P: AsRef<Path>>(
        data_dir: P,
        network: Network,
        rpc_url: &Url,
        auth: Auth,
    ) -> Result<Brc721Wallet> {
        let local = LocalWallet::load(&data_dir, network)?;
        let remote = RemoteWallet::new(local.id(), rpc_url, auth);
        Ok(Self {
            local,
            remote,
            signer: Signer::new().with_data_dir(&data_dir).with_network(network),
        })
    }

    pub fn id(&self) -> String {
        self.local.id()
    }

    pub fn reveal_next_payment_address(&mut self) -> Result<AddressInfo> {
        self.local.reveal_next_payment_address()
    }

    pub fn balances(&self) -> Result<json::GetBalancesResult> {
        self.remote.balances()
    }

    pub fn rescan_watch_only(&self) -> Result<()> {
        self.remote.rescan()
    }

    pub fn setup_watch_only(&self) -> Result<()> {
        let external = self.local.public_descriptor(KeychainKind::External);
        let internal = self.local.public_descriptor(KeychainKind::Internal);
        self.remote.setup(external, internal)
    }

    /// Send `amount` to `target_address` using funds from this wallet.
    ///
    /// This creates a PSBT via the Core watch-only wallet, signs it with BDK private keys,
    /// finalizes, and broadcasts it.
    ///
    /// Arguments:
    /// - `rpc_url`: The node RPC url (usually http[s]://host:port).
    /// - `auth`: Auth credentials for the node.
    /// - `target_address`: Address to receive funds.
    /// - `amount`: Amount to send.
    /// - `fee_rate`: Optional sats/vB feerate.
    ///
    /// Returns the finalized transaction (not broadcast).
    pub fn build_payment_tx(
        &self,
        target_address: &Address,
        amount: Amount,
        fee_rate: Option<f64>,
        passphrase: String,
    ) -> Result<bitcoin::Transaction> {
        let mut psbt: Psbt =
            self.remote
                .create_psbt_for_payment(target_address, amount, fee_rate)?;

        let finalized = self
            .signer
            .sign(&mut psbt, &age::secrecy::SecretString::from(passphrase))
            .context("bdk sign")?;

        let secp = bitcoin::secp256k1::Secp256k1::verification_only();
        if !finalized {
            psbt.finalize_mut(&secp)
                .map_err(|errs| anyhow::anyhow!("finalize_mut: {:?}", errs))?;
        }
        let tx = psbt
            .extract(&secp)
            .map_err(|e| anyhow::anyhow!("extract_tx: {e}"))?;

        Ok(tx)
    }

    pub fn build_tx(
        &self,
        outputs: Vec<bitcoin::TxOut>,
        fee_rate: Option<f64>,
        passphrase: String,
    ) -> Result<bitcoin::Transaction> {
        let mut psbt = self
            .remote
            .create_psbt_from_txouts(outputs, fee_rate)
            .context("create psbt from outputs")?;

        let finalized = self
            .signer
            .sign(&mut psbt, &age::secrecy::SecretString::from(passphrase))
            .context("bdk sign")?;

        let secp = bitcoin::secp256k1::Secp256k1::verification_only();
        if !finalized {
            psbt.finalize_mut(&secp)
                .map_err(|errs| anyhow::anyhow!("finalize_mut: {:?}", errs))?;
        }
        let tx = psbt
            .extract(&secp)
            .map_err(|e| anyhow::anyhow!("extract_tx: {e}"))?;

        Ok(tx)
    }

    pub fn broadcast(&self, tx: &bitcoin::Transaction) -> Result<bitcoin::Txid> {
        self.remote.broadcast(tx)
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
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let wallet = Brc721Wallet::create(
            &data_dir,
            Network::Regtest,
            Some(mnemonic),
            "passphrase".to_string(),
            &node_url,
            auth,
        )
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
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let mut wallet = Brc721Wallet::create(
            &data_dir,
            network,
            Some(mnemonic.clone()),
            "passphrase".to_string(),
            &node_url,
            auth,
        )
        .expect("create wallet");
        let addr1 = wallet
            .reveal_next_payment_address()
            .expect("address")
            .address;

        // Reload the wallet from storage
        let mut loaded_wallet = Brc721Wallet::load(
            &data_dir,
            network,
            &node_url,
            bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone()),
        )
        .expect("load should not fail");

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
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let mut wallet = Brc721Wallet::create(
            &data_dir,
            network,
            Some(mnemonic),
            "passphrase".to_string(),
            &node_url,
            auth,
        )
        .expect("wallet");
        let address_info = wallet.reveal_next_payment_address().expect("address");
        // Ensure the address is not empty
        assert_eq!(
            address_info.address.to_string(),
            "bcrt1p8wpt9v4frpf3tkn0srd97pksgsxc5hs52lafxwru9kgeephvs7rqjeprhg"
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
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let mut wallet = Brc721Wallet::create(
            &data_dir,
            network,
            Some(mnemonic),
            "passphrase".to_string(),
            &node_url,
            auth,
        )
        .expect("wallet");
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
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let result = Brc721Wallet::load(&data_dir, Network::Regtest, &node_url, auth);
        assert!(
            result.is_err(),
            "Expected an error when loading a wallet that doesn't exist"
        );
    }

    #[test]
    fn test_wallet_id_uniqueness_across_networks() {
        let data_dir0 = TempDir::new().expect("temp dir");
        let data_dir1 = TempDir::new().expect("temp dir");

        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let wallet_regtest = Brc721Wallet::create(
            &data_dir0,
            Network::Regtest,
            Some(mnemonic.clone()),
            "passphrase".to_string(),
            &node_url,
            auth.clone(),
        )
        .expect("regtest");
        let wallet_bitcoin = Brc721Wallet::create(
            &data_dir1,
            Network::Bitcoin,
            Some(mnemonic),
            "passphrase".to_string(),
            &node_url,
            auth,
        )
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
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let wallet0 = Brc721Wallet::create(
            &data_dir0,
            Network::Regtest,
            Some(mnemonic.clone()),
            "passphrase".to_string(),
            &node_url,
            auth.clone(),
        )
        .expect("wallet0");

        let data_dir1 = TempDir::new().expect("temp dir");
        let wallet1 = Brc721Wallet::create(
            &data_dir1,
            Network::Regtest,
            Some(mnemonic.clone()),
            "passphrase".to_string(),
            &node_url,
            auth,
        )
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
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let wallet0 = Brc721Wallet::create(
            &data_dir0,
            Network::Regtest,
            Some(mnemonic.clone()),
            "passphrase1".to_string(),
            &node_url,
            auth.clone(),
        )
        .expect("wallet0");

        let data_dir1 = TempDir::new().expect("temp dir");
        let wallet1 = Brc721Wallet::create(
            &data_dir1,
            Network::Regtest,
            Some(mnemonic.clone()),
            "passphrase1".to_string(),
            &node_url,
            auth,
        )
        .expect("wallet1");

        assert_eq!(
            wallet0.id(),
            wallet1.id(),
            "Wallet id should be stable for same mnemonic, network and passphrase"
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
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        Brc721Wallet::create(
            &data_dir,
            network,
            Some(mnemonic),
            "passphrase".to_string(),
            &node_url,
            auth.clone(),
        )
        .expect("wallet");
        let wallet = Brc721Wallet::load(&data_dir, network, &node_url, auth).expect("wallet");
        assert!(!wallet.id().is_empty());
    }

    #[test]
    fn test_regtest_wallet_persist_on_storage() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        Brc721Wallet::create(
            &data_dir,
            Network::Regtest,
            Some(mnemonic),
            "passphrase".to_string(),
            &node_url,
            auth,
        )
        .expect("wallet");
        let expected_wallet_path = data_dir.path().join("wallet.sqlite");
        assert!(expected_wallet_path.exists());
    }

    #[test]
    fn test_bitcoin_wallet_persist_on_storage() {
        let data_dir = TempDir::new().expect("temp dir");
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).expect("mnemonic");

        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        Brc721Wallet::create(
            &data_dir,
            Network::Bitcoin,
            Some(mnemonic),
            "passphrase".to_string(),
            &node_url,
            auth,
        )
        .expect("wallet");
        let expected_wallet_path = data_dir.path().join("wallet.sqlite");
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
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        Brc721Wallet::create(
            &data_dir,
            Network::Regtest,
            Some(mnemonic.clone()),
            "passphrase".to_string(),
            &node_url,
            auth.clone(),
        )
        .expect("first wallet");
        // Second creation should error because the db is already there
        let result = Brc721Wallet::create(
            &data_dir,
            Network::Regtest,
            Some(mnemonic),
            "passphrase".to_string(),
            &node_url,
            auth.clone(),
        );
        assert!(
            result.is_err(),
            "Expected an error when re-creating the wallet"
        );
    }
}
