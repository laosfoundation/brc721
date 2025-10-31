use bdk_wallet::keys::bip39::Mnemonic;
use bitcoin::Amount;
use bitcoincore_rpc::{Auth, RpcApi};
use std::path::PathBuf;

pub struct InitResult {
    pub created: bool,
    pub mnemonic: Option<Mnemonic>,
    pub db_path: PathBuf,
}

pub struct CoreWalletInfo {
    pub name: String,
    pub watch_only: bool,
    pub descriptors: bool,
}

pub trait CoreRpc {
    fn list_wallets(&self) -> anyhow::Result<Vec<String>>;
    fn get_wallet_info(&self, name: &str) -> anyhow::Result<serde_json::Value>;
    fn get_wallet_balance(&self, name: &str) -> anyhow::Result<Amount>;
}

pub struct RealCoreRpc {
    pub base_url: String,
    pub auth: Auth,
}

impl RealCoreRpc {
    pub fn new(base_url: String, auth: Auth) -> Self {
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

pub trait CoreAdmin {
    fn ensure_watchonly_descriptor_wallet(&self, wallet_name: &str) -> anyhow::Result<()>;
    fn import_descriptors(
        &self,
        wallet_name: &str,
        imports: serde_json::Value,
    ) -> anyhow::Result<()>;
}

pub struct RealCoreAdmin {
    pub base_url: String,
    pub auth: Auth,
}

impl RealCoreAdmin {
    pub fn new(base_url: String, auth: Auth) -> Self {
        Self { base_url, auth }
    }

    fn root_client(&self) -> anyhow::Result<bitcoincore_rpc::Client> {
        let cli = bitcoincore_rpc::Client::new(&self.base_url, self.auth.clone())?;
        Ok(cli)
    }

    fn wallet_client(&self, name: &str) -> anyhow::Result<bitcoincore_rpc::Client> {
        let wallet_url = format!("{}/wallet/{}", self.base_url.trim_end_matches('/'), name);
        let cli = bitcoincore_rpc::Client::new(&wallet_url, self.auth.clone())?;
        Ok(cli)
    }
}

impl CoreAdmin for RealCoreAdmin {
    fn ensure_watchonly_descriptor_wallet(&self, wallet_name: &str) -> anyhow::Result<()> {
        let root = self.root_client()?;
        let res: Result<serde_json::Value, bitcoincore_rpc::Error> = root.call(
            "createwallet",
            &[
                serde_json::json!(wallet_name),
                serde_json::json!(true),  // disable_private_keys
                serde_json::json!(true),  // blank
                serde_json::json!(""),    // passphrase
                serde_json::json!(false), // avoid_reuse
                serde_json::json!(true),  // descriptors
            ],
        );
        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                if self
                    .wallet_client(wallet_name)
                    .and_then(|cli| {
                        let r: Result<serde_json::Value, bitcoincore_rpc::Error> =
                            cli.call("getwalletinfo", &[]);
                        r.map(|_| ()).map_err(|err| anyhow::anyhow!(err))
                    })
                    .is_ok()
                {
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    fn import_descriptors(
        &self,
        wallet_name: &str,
        imports: serde_json::Value,
    ) -> anyhow::Result<()> {
        let cli = self.wallet_client(wallet_name)?;
        let _res: serde_json::Value = cli.call("importdescriptors", &[imports])?;
        Ok(())
    }
}
