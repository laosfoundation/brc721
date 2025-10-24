use bdk_wallet::keys::bip39::Mnemonic;
use bitcoin::Amount;
use std::path::PathBuf;
use bitcoincore_rpc::RpcApi;

pub struct InitResult {
    pub created: bool,
    pub mnemonic: Option<Mnemonic>,
    pub db_path: PathBuf,
}

pub trait CoreRpc {
    fn list_wallets(&self) -> anyhow::Result<Vec<String>>;
    fn get_wallet_info(&self, name: &str) -> anyhow::Result<serde_json::Value>;
    fn get_wallet_balance(&self, name: &str) -> anyhow::Result<Amount>;
}

pub struct RealCoreRpc {
    pub base_url: String,
    pub auth: bitcoincore_rpc::Auth,
}

impl RealCoreRpc {
    pub fn new(base_url: String, auth: bitcoincore_rpc::Auth) -> Self {
        Self { base_url, auth }
    }

    fn client_for_wallet(&self, name: &str) -> anyhow::Result<bitcoincore_rpc::Client> {
        let wallet_url = format!("{}/wallet/{}", self.base_url.trim_end_matches('/'), name);
        let cli = bitcoincore_rpc::Client::new(&wallet_url, self.auth.clone())?;
        Ok(cli)
    }

    fn root_client(&self) -> anyhow::Result<bitcoincore_rpc::Client> {
        let cli = bitcoincore_rpc::Client::new(&self.base_url, self.auth.clone())?;
        Ok(cli)
    }
}

impl CoreRpc for RealCoreRpc {
    fn list_wallets(&self) -> anyhow::Result<Vec<String>> {
        let root = self.root_client()?;
        let v = root.list_wallets()?;
        Ok(v)
    }

    fn get_wallet_info(&self, name: &str) -> anyhow::Result<serde_json::Value> {
        let cli = self.client_for_wallet(name)?;
        let info: serde_json::Value = cli.call("getwalletinfo", &[])?;
        Ok(info)
    }

    fn get_wallet_balance(&self, name: &str) -> anyhow::Result<Amount> {
        let cli = self.client_for_wallet(name)?;
        let bal = cli.get_balance(None, None)?;
        Ok(bal)
    }
}
