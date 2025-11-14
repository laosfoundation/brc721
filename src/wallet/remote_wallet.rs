use anyhow::{Context, Result};
use bitcoin::{Address, Transaction, TxOut};
use bitcoin::{Amount, Psbt};
use bitcoincore_rpc::{json, Auth, Client, RpcApi};
use url::Url;

pub struct RemoteWallet {
    watch_name: String,
    rpc_url: Url,
    auth: Auth,
}

impl RemoteWallet {
    pub fn new(watch_name: String, rpc_url: &Url, auth: Auth) -> Self {
        Self {
            watch_name,
            rpc_url: rpc_url.clone(),
            auth,
        }
    }

    pub fn detect_network(rpc_url: &Url, auth: &Auth) -> Result<bitcoin::Network> {
        let client = Client::new(rpc_url.as_ref(), auth.clone()).context("create root client")?;
        let info = client
            .get_blockchain_info()
            .context("get_blockchain_info")?;
        Ok(info.chain)
    }

    fn watch_url(&self) -> String {
        format!(
            "{}/wallet/{}",
            self.rpc_url.to_string().trim_end_matches('/'),
            self.watch_name
        )
    }

    fn watch_client(&self) -> Result<Client> {
        let url = self.watch_url();
        Client::new(&url, self.auth.clone()).context("create Core wallet client")
    }

    fn root_client(&self) -> Result<Client> {
        Client::new(self.rpc_url.as_ref(), self.auth.clone()).context("create root client")
    }

    pub fn balances(&self) -> Result<json::GetBalancesResult> {
        let client = self.watch_client()?;
        client.get_balances().context("get balance")
    }

    pub fn rescan(&self) -> Result<()> {
        let client = self.watch_client()?;
        let mut params = Vec::new();
        let start_block = 0;
        params.push(serde_json::json!(start_block));
        let _ans: serde_json::Value = client
            .call::<serde_json::Value>("rescanblockchain", &params)
            .context("rescanblockchain")?;
        Ok(())
    }

    pub fn setup(&self, external_desc: String, internal_desc: String) -> Result<()> {
        let root = self.root_client()?;
        let existing_wallets: Vec<String> = root.list_wallets().context("list wallets")?;
        if existing_wallets.contains(&self.watch_name) {
            return Ok(());
        }
        let ans: serde_json::Value = root
            .call::<serde_json::Value>(
                "createwallet",
                &[
                    serde_json::json!(self.watch_name), // Wallet name
                    serde_json::json!(true),            // Disable private keys
                    serde_json::json!(true),            // Blank wallet
                    serde_json::json!(""),              // Passphrase
                    serde_json::json!(false),           // avoid_reuse
                    serde_json::json!(true),            // Descriptors enabled
                ],
            )
            .context("watch wallet created")?;
        if ans["name"].as_str().unwrap() != self.watch_name {
            return Err(anyhow::anyhow!("Unexpected wallet name: {:?}", ans["name"]));
        }
        if !ans["warning"].is_null() {
            return Err(anyhow::anyhow!(
                "watch_only wallet created with warning: {}",
                ans["warning"]
            ));
        }

        let client = self.watch_client()?;
        let imports = serde_json::json!([
            {
                "desc": external_desc,
                "timestamp": "now",
                "active": true,
                "range": [0,999],
                "internal": false
            },
            {
                "desc": internal_desc,
                "timestamp": "now",
                "active": true,
                "range": [0,999],
                "internal": true
            }
        ]);
        let ans: serde_json::Value = client
            .call::<serde_json::Value>("importdescriptors", &[imports])
            .context("import descriptor")?;

        let arr = ans.as_array().expect("array");
        if !arr.iter().all(|e| e["success"].as_bool() == Some(true)) {
            return Err(anyhow::anyhow!(
                "Failed to import descriptors: {}",
                serde_json::to_string_pretty(&ans).unwrap()
            ));
        }
        Ok(())
    }

    pub fn create_psbt_for_payment(
        &self,
        target_address: &Address,
        amount: Amount,
        fee_rate: Option<f64>,
    ) -> Result<Psbt> {
        let client = self.watch_client()?;
        let outputs = serde_json::json!([{ target_address.to_string(): amount.to_btc() }]);
        let mut options = serde_json::json!({});
        if let Some(fr) = fee_rate {
            options["fee_rate"] = serde_json::json!(fr);
        }
        let funded: serde_json::Value = client
            .call(
                "walletcreatefundedpsbt",
                &[
                    serde_json::json!([]),
                    outputs,
                    serde_json::json!(0),
                    options,
                    serde_json::json!(true),
                ],
            )
            .context("walletcreatefundedpsbt")?;
        let psbt_b64 = funded["psbt"].as_str().context("psbt base64")?;
        let psbt: Psbt = psbt_b64.parse().context("parse psbt base64")?;
        Ok(psbt)
    }

    pub fn create_psbt_from_txouts(
        &self,
        outputs: Vec<TxOut>,
        fee_rate: Option<f64>,
    ) -> Result<Psbt> {
        let client = self.watch_client()?;
        let network = Self::detect_network(&self.rpc_url, &self.auth)?;

        let outs: Vec<serde_json::Value> = outputs
            .into_iter()
            .map(|o| {
                if let Ok(addr) = Address::from_script(&o.script_pubkey, network) {
                    serde_json::json!({ addr.to_string(): o.value.to_btc() })
                } else {
                    serde_json::json!({
                        "script": hex::encode(o.script_pubkey.as_bytes()),
                        "amount": o.value.to_btc()
                    })
                }
            })
            .collect();

        let raw_hex: String = client
            .call(
                "createrawtransaction",
                &[serde_json::json!([]), serde_json::json!(outs)],
            )
            .context("createrawtransaction")?;

        let mut options = serde_json::json!({});
        if let Some(fr) = fee_rate {
            options["feeRate"] = serde_json::json!(fr);
        }

        let funded: serde_json::Value = client
            .call("fundrawtransaction", &[serde_json::json!(raw_hex), options])
            .context("fundrawtransaction")?;

        let funded_hex = funded["hex"].as_str().context("funded hex")?;

        let psbt_b64: String = client
            .call(
                "converttopsbt",
                &[serde_json::json!(funded_hex), serde_json::json!(true)],
            )
            .context("converttopsbt")?;

        let psbt: Psbt = psbt_b64.parse().context("parse psbt base64")?;
        Ok(psbt)
    }

    pub fn broadcast(&self, tx: &Transaction) -> Result<bitcoin::Txid> {
        let root = self.root_client()?;
        let txid = root.send_raw_transaction(tx).context("broadcast tx")?;
        Ok(txid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bdk_wallet::{
        bip39::{Language, Mnemonic},
        template::Bip86,
        KeychainKind, Wallet,
    };
    use bitcoin::{
        bip32::Xpriv,
        opcodes,
        script::{Builder, PushBytesBuf},
        Network,
    };

    fn create_wallet() -> Wallet {
        // Parse the deterministic 12-word BIP39 mnemonic seed phrase.
        let mnemonic = Mnemonic::parse_in(
            Language::English,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        ).unwrap();
        let network = Network::Regtest;

        // Derive BIP32 master private key from seed.
        let seed = mnemonic.to_seed(String::new()); // empty password
        let master_xprv = Xpriv::new_master(network, &seed).expect("master_key");

        // Initialize the wallet using BIP86 descriptors for both keychains.
        Wallet::create(
            Bip86(master_xprv, KeychainKind::External),
            Bip86(master_xprv, KeychainKind::Internal),
        )
        .network(network)
        .create_wallet_no_persist()
        .expect("wallet")
    }

    #[test]
    fn check_psbt_by_outputs() {
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let mut wallet = create_wallet();
        let remote_wallet = RemoteWallet::new("watch_only".to_string(), &node_url, auth.clone());
        let addr = wallet.reveal_next_address(KeychainKind::External);
        let external = wallet.public_descriptor(KeychainKind::External).to_string();
        let internal = wallet.public_descriptor(KeychainKind::Internal).to_string();
        remote_wallet
            .setup(external, internal)
            .expect("remove wallet setup");

        let root = bitcoincore_rpc::Client::new(&node.rpc_url(), auth.clone()).unwrap();
        root.generate_to_address(101, &addr.address).expect("mint");

        let output = TxOut {
            value: Amount::from_sat(1000),
            script_pubkey: addr.script_pubkey(),
        };

        remote_wallet
            .create_psbt_from_txouts(vec![output], None)
            .expect("psbt");
    }

    #[test]
    fn check_psbt_by_output_using_op_return_script() {
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let mut wallet = create_wallet();
        let remote_wallet = RemoteWallet::new("watch_only".to_string(), &node_url, auth.clone());
        let addr = wallet.reveal_next_address(KeychainKind::External);
        let external = wallet.public_descriptor(KeychainKind::External).to_string();
        let internal = wallet.public_descriptor(KeychainKind::Internal).to_string();
        remote_wallet
            .setup(external, internal)
            .expect("remove wallet setup");

        let root = bitcoincore_rpc::Client::new(&node.rpc_url(), auth.clone()).unwrap();
        root.generate_to_address(101, &addr.address).expect("mint");

        let payload = vec![0x0a];
        let pb = PushBytesBuf::try_from(payload.to_vec()).unwrap();
        let script = Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_opcode(opcodes::all::OP_PUSHNUM_15)
            .push_slice(pb)
            .into_script();
        let output = TxOut {
            value: Amount::from_sat(0),
            script_pubkey: script,
        };

        remote_wallet
            .create_psbt_from_txouts(vec![output], None)
            .expect("psbt");
    }
}
