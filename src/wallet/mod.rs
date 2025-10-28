pub mod paths;
pub mod types;

use anyhow::{anyhow, Context, Result};
use bdk_bitcoind_rpc::{Emitter, NO_EXPECTED_MEMPOOL_TXS};
use bdk_wallet::{
    keys::bip39::{Language, Mnemonic, WordCount},
    template::Bip86,
    Balance, CreateParams, KeychainKind, LoadParams, PersistedWallet,
};
use bitcoin::Transaction;
use bitcoin::{Address, Network};
use bitcoincore_rpc::Auth;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;

use paths::wallet_db_path;

#[derive(Debug)]
pub struct Wallet {
    data_dir: PathBuf,
    network: Network,
}

impl Wallet {
    pub fn new<P: Into<PathBuf>>(data_dir: P, network: Network) -> Self {
        Self {
            data_dir: data_dir.into(),
            network,
        }
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

    pub fn balance(&self) -> Result<Balance> {
        let wallet = self.load_wallet_or_err()?;
        Ok(wallet.balance())
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

    pub fn sync(&self, rpc_url: &str, auth: Auth) -> Result<()> {
        let mut wallet = self.load_wallet_or_err()?;
        let wallet_tip = wallet.latest_checkpoint();
        log::info!("sync: connecting to {}", rpc_url);
        let client = bitcoincore_rpc::Client::new(rpc_url, auth)?;
        log::info!(
            "sync: starting from checkpoint height {}",
            wallet_tip.height()
        );
        let mut conn = self.open_conn()?;
        let mut emitter = Emitter::new(
            &client,
            wallet_tip.clone(),
            wallet_tip.height(),
            NO_EXPECTED_MEMPOOL_TXS,
        );

        let mut applied_blocks: u64 = 0;
        let mut last_height = wallet_tip.height();
        while let Some(block) = emitter.next_block()? {
            log::info!("{}", block.block_height());
            last_height = block.block_height();
            wallet.apply_block_connected_to(
                &block.block,
                block.block_height(),
                block.connected_to(),
            )?;
            wallet.persist(&mut conn)?;
            applied_blocks += 1;
        }
        log::info!(
            "sync: applied {} blocks, new tip height {}",
            applied_blocks,
            last_height
        );

        let mempool_emissions: Vec<(Arc<Transaction>, u64)> = emitter.mempool()?.update;
        let mempool_count = mempool_emissions.len();
        wallet.apply_unconfirmed_txs(mempool_emissions);
        log::info!("sync: applied {} mempool txs", mempool_count);

        wallet.persist(&mut conn)?;
        let bal = wallet.balance();
        log::info!("sync: balance {bal}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{distributions::Alphanumeric, Rng};

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
    fn public_descriptors_error_if_uninitialized() {
        let dir = temp_data_dir();
        let w = Wallet::new(&dir, bitcoin::Network::Regtest);
        let res = w.public_descriptors_with_checksum();
        assert!(res.is_err(), "should error when wallet not initialized");
    }
}
