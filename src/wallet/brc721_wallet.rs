use super::{local_wallet::LocalWallet, remote_wallet::RemoteWallet, signer::Signer};
use age::secrecy::SecretString;
use anyhow::{Context, Result};
use bdk_wallet::template::Bip86;
use bdk_wallet::{bip39::Mnemonic, miniscript::psbt::PsbtExt, AddressInfo, KeychainKind};
use bitcoin::{bip32::Xpriv, Address, Amount, Network, OutPoint, Psbt};
use bitcoincore_rpc::json;
use bitcoincore_rpc::Auth;
use std::path::Path;
use url::Url;

pub struct Brc721Wallet {
    local: LocalWallet,
    remote: RemoteWallet,
    signer: Signer,
}

impl Brc721Wallet {
    pub fn create<P: AsRef<Path>>(
        data_dir: P,
        network: Network,
        mnemonic: Mnemonic,
        passphrase: SecretString,
        rpc_url: &Url,
        auth: Auth,
    ) -> Result<Brc721Wallet> {
        let seed = mnemonic.to_seed(String::default());
        let master_xprv = Xpriv::new_master(network, &seed).expect("master_key");
        let external = Bip86(master_xprv, KeychainKind::External);
        let internal = Bip86(master_xprv, KeychainKind::Internal);

        let local = LocalWallet::create(&data_dir, network, external, internal)?;
        let remote = RemoteWallet::new(local.id(), rpc_url, auth);

        let signer = Signer::new(&data_dir, network);
        signer.store_master_key(&master_xprv, &passphrase)?;

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
        remote
            .load_wallet()
            .context("load remote bitcoin core wallet")?;
        Ok(Self {
            local,
            remote,
            signer: Signer::new(&data_dir, network),
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

    pub fn list_unspent(&self, min_conf: u64) -> Result<Vec<json::ListUnspentResultEntry>> {
        self.remote.list_unspent(min_conf)
    }

    pub fn rescan_watch_only(&self) -> Result<()> {
        self.remote.rescan()
    }

    pub fn load_watch_only(&self) -> Result<()> {
        self.remote.load_wallet()
    }

    pub fn unload_watch_only(&self) -> Result<()> {
        self.remote.unload_wallet()
    }

    pub fn loaded_core_wallets(&self) -> Result<Vec<String>> {
        self.remote.loaded_wallets()
    }

    pub fn setup_watch_only(&self) -> Result<()> {
        let external = self.local.public_descriptor(KeychainKind::External);
        let internal = self.local.public_descriptor(KeychainKind::Internal);
        self.remote.setup(external, internal)
    }

    pub fn build_payment_tx(
        &self,
        target_address: &Address,
        amount: Amount,
        fee_rate: Option<f64>,
        passphrase: SecretString,
    ) -> Result<bitcoin::Transaction> {
        let psbt: Psbt = self
            .remote
            .create_psbt_for_payment(target_address, amount, fee_rate)?;

        self.sign(psbt, &passphrase)
    }

    pub fn build_tx(
        &self,
        output: bitcoin::TxOut,
        fee_rate: Option<f64>,
        passphrase: SecretString,
    ) -> Result<bitcoin::Transaction> {
        let psbt = self
            .remote
            .create_psbt_from_txout(output, fee_rate)
            .context("create psbt from outputs")?;

        self.sign(psbt, &passphrase)
    }

    pub fn build_tx_with_op_return_and_payments(
        &self,
        op_return: bitcoin::TxOut,
        payments: Vec<(Address, Amount)>,
        fee_rate: Option<f64>,
        passphrase: SecretString,
    ) -> Result<bitcoin::Transaction> {
        let psbt = self
            .remote
            .create_psbt_from_opreturn_and_payments(op_return, payments, fee_rate)
            .context("create psbt from op_return + payments")?;

        self.sign(psbt, &passphrase)
    }

    pub fn broadcast(&self, tx: &bitcoin::Transaction) -> Result<bitcoin::Txid> {
        self.remote.broadcast(tx)
    }

    pub fn build_implicit_transfer_tx(
        &self,
        token_outpoints: &[OutPoint],
        target_address: &Address,
        amount_per_output: Amount,
        fee_rate: Option<f64>,
        lock_outpoints: &[OutPoint],
        passphrase: SecretString,
    ) -> Result<bitcoin::Transaction> {
        let locked = self.remote.list_locked_unspent()?;
        let locked_spending = token_outpoints
            .iter()
            .filter(|outpoint| locked.contains(outpoint))
            .cloned()
            .collect::<Vec<_>>();
        if !locked_spending.is_empty() {
            return Err(anyhow::anyhow!(
                "cannot spend locked outpoints: {}",
                locked_spending
                    .iter()
                    .map(|op| format!("{}:{}", op.txid, op.vout))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        let to_lock = lock_outpoints
            .iter()
            .filter(|outpoint| !locked.contains(outpoint))
            .cloned()
            .collect::<Vec<_>>();

        self.remote
            .lock_unspent_outpoints(&to_lock)
            .context("lock token outpoints")?;

        let psbt_res = self.remote.create_psbt_for_implicit_transfer(
            token_outpoints,
            target_address,
            amount_per_output,
            fee_rate,
        );

        let unlock_res = self.remote.unlock_unspent_outpoints(&to_lock);
        if let Err(unlock_err) = unlock_res {
            log::warn!("Failed to unlock outpoints: {unlock_err:#}");
        }

        let psbt = psbt_res.context("create implicit transfer PSBT")?;
        self.sign(psbt, &passphrase)
    }

    pub fn build_mix_tx(
        &self,
        token_outpoints: &[OutPoint],
        op_return: bitcoin::TxOut,
        payments: Vec<(Address, Amount)>,
        fee_rate: Option<f64>,
        passphrase: SecretString,
    ) -> Result<bitcoin::Transaction> {
        if token_outpoints.is_empty() {
            return Err(anyhow::anyhow!("mix requires at least one input"));
        }

        let locked = self.remote.list_locked_unspent()?;
        let locked_spending = token_outpoints
            .iter()
            .filter(|outpoint| locked.contains(outpoint))
            .cloned()
            .collect::<Vec<_>>();
        if !locked_spending.is_empty() {
            return Err(anyhow::anyhow!(
                "cannot spend locked outpoints: {}",
                locked_spending
                    .iter()
                    .map(|op| format!("{}:{}", op.txid, op.vout))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        let psbt = self
            .remote
            .create_psbt_for_mix(token_outpoints, op_return, payments, fee_rate)
            .context("create mix PSBT")?;

        self.sign(psbt, &passphrase)
    }

    fn sign(&self, mut psbt: Psbt, passphrase: &SecretString) -> Result<bitcoin::Transaction> {
        let finalized = self
            .signer
            .sign(&mut psbt, passphrase)
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
            mnemonic,
            SecretString::from("passphrase".to_string()),
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
            mnemonic.clone(),
            SecretString::from("passphrase".to_string()),
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
            mnemonic,
            SecretString::from("passphrase".to_string()),
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
            mnemonic,
            SecretString::from("passphrase".to_string()),
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
            mnemonic.clone(),
            SecretString::from("passphrase".to_string()),
            &node_url,
            auth.clone(),
        )
        .expect("regtest");
        let wallet_bitcoin = Brc721Wallet::create(
            &data_dir1,
            Network::Bitcoin,
            mnemonic,
            SecretString::from("passphrase".to_string()),
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
            mnemonic.clone(),
            SecretString::from("passphrase".to_string()),
            &node_url,
            auth.clone(),
        )
        .expect("wallet0");

        let data_dir1 = TempDir::new().expect("temp dir");
        let wallet1 = Brc721Wallet::create(
            &data_dir1,
            Network::Regtest,
            mnemonic.clone(),
            SecretString::from("passphrase".to_string()),
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
            mnemonic.clone(),
            SecretString::from("passphrase1".to_string()),
            &node_url,
            auth.clone(),
        )
        .expect("wallet0");

        let data_dir1 = TempDir::new().expect("temp dir");
        let wallet1 = Brc721Wallet::create(
            &data_dir1,
            Network::Regtest,
            mnemonic.clone(),
            SecretString::from("passphrase1".to_string()),
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
            mnemonic,
            SecretString::from("passphrase".to_string()),
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
            mnemonic,
            SecretString::from("passphrase".to_string()),
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
            mnemonic,
            SecretString::from("passphrase".to_string()),
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
            mnemonic.clone(),
            SecretString::from("passphrase".to_string()),
            &node_url,
            auth.clone(),
        )
        .expect("first wallet");
        // Second creation should error because the db is already there
        let result = Brc721Wallet::create(
            &data_dir,
            Network::Regtest,
            mnemonic,
            SecretString::from("passphrase".to_string()),
            &node_url,
            auth.clone(),
        );
        assert!(
            result.is_err(),
            "Expected an error when re-creating the wallet"
        );
    }
}
