use anyhow::{Context, Result};
use bitcoin::{Address, ScriptBuf, Transaction, TxOut};
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

    pub fn create_psbt_from_txout(&self, output: TxOut, fee_rate: Option<f64>) -> Result<Psbt> {
        let client = self.watch_client()?;

        let script = output.script_pubkey;
        let dummy = bitcoin::script::ScriptBuf::from(vec![0u8; script.len() - 2]);
        let mut options = serde_json::json!({});
        if let Some(fr) = fee_rate {
            options["fee_rate"] = serde_json::json!(fr);
        }
        let funded: serde_json::Value = client
            .call(
                "walletcreatefundedpsbt",
                &[
                    serde_json::json!([]),
                    serde_json::json!([{"data": dummy}]),
                    serde_json::json!(0),
                    options,
                    serde_json::json!(true),
                ],
            )
            .context("walletcreatefundedpsbt from raw tx data")?;
        let psbt_b64 = funded["psbt"].as_str().context("psbt base64")?;
        let mut psbt: Psbt = psbt_b64.parse().context("parse psbt base64")?;

        psbt = substitute_first_opreturn_script(psbt, script).context("dummy not found")?;
        psbt = move_opreturn_first(psbt);
        Ok(psbt)
    }

    pub fn broadcast(&self, tx: &Transaction) -> Result<bitcoin::Txid> {
        let root = self.root_client()?;
        let txid = root.send_raw_transaction(tx).context("broadcast tx")?;
        Ok(txid)
    }

    fn watch_client(&self) -> Result<Client> {
        let url = format!(
            "{}/wallet/{}",
            self.rpc_url.to_string().trim_end_matches('/'),
            self.watch_name
        );
        Client::new(&url, self.auth.clone()).context("create Core wallet client")
    }

    fn root_client(&self) -> Result<Client> {
        Client::new(self.rpc_url.as_ref(), self.auth.clone()).context("create root client")
    }
}

fn move_opreturn_first(mut psbt: Psbt) -> Psbt {
    let opret_index_opt = psbt
        .unsigned_tx
        .output
        .iter()
        .position(|txout| txout.script_pubkey.is_op_return());

    let Some(opret_index) = opret_index_opt else {
        return psbt;
    };

    if opret_index == 0 {
        return psbt;
    }

    // Reorder unsigned_tx.output
    let mut new_tx_outputs = Vec::with_capacity(psbt.unsigned_tx.output.len());
    let opret_txout = psbt.unsigned_tx.output[opret_index].clone();
    new_tx_outputs.push(opret_txout);

    for (i, txout) in psbt.unsigned_tx.output.iter().enumerate() {
        if i != opret_index {
            new_tx_outputs.push(txout.clone());
        }
    }
    psbt.unsigned_tx.output = new_tx_outputs;

    // Reorder PSBT outputs metadata consistently
    let mut new_psbt_outputs = Vec::with_capacity(psbt.outputs.len());
    let opret_meta = psbt.outputs[opret_index].clone();
    new_psbt_outputs.push(opret_meta);

    for (i, out_meta) in psbt.outputs.iter().enumerate() {
        if i != opret_index {
            new_psbt_outputs.push(out_meta.clone());
        }
    }
    psbt.outputs = new_psbt_outputs;

    psbt
}

/// Substitute the script of the first OP_RETURN output in a PSBT while keeping the amount unchanged.
/// If no OP_RETURN output exists, the PSBT is returned unchanged.
fn substitute_first_opreturn_script(mut psbt: Psbt, new_script: ScriptBuf) -> Result<Psbt> {
    let idx_opt = psbt
        .unsigned_tx
        .output
        .iter()
        .position(|txout| txout.script_pubkey.is_op_return());

    let Some(idx) = idx_opt else {
        return Ok(psbt);
    };

    let original_amount = psbt.unsigned_tx.output[idx].value;
    psbt.unsigned_tx.output[idx].script_pubkey = new_script.clone();
    psbt.unsigned_tx.output[idx].value = original_amount;

    Ok(psbt)
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
            .create_psbt_from_txout(output, None)
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

        let payload = [0x0a];
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

        assert_eq!(output.script_pubkey.len(), 4);

        let psbt = remote_wallet
            .create_psbt_from_txout(output.clone(), None)
            .expect("psbt");

        assert_eq!(psbt.unsigned_tx.output.len(), 2);
        assert_eq!(
            psbt.unsigned_tx.output[0].clone().script_pubkey,
            output.script_pubkey
        );
    }
}
