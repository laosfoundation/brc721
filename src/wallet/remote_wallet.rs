use anyhow::{Context, Result};
use bitcoin::Psbt;
use bitcoin::{Address, Amount, OutPoint, ScriptBuf, Transaction, TxOut};
use bitcoincore_rpc::{json, Auth, Client, RpcApi};
use serde::Deserialize;
use std::collections::BTreeSet;
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

    pub fn list_unspent(&self, min_conf: u64) -> Result<Vec<json::ListUnspentResultEntry>> {
        let client = self.watch_client()?;
        let min_conf: usize = min_conf
            .try_into()
            .map_err(|_| anyhow::anyhow!("min-conf out of range: {}", min_conf))?;
        client
            .list_unspent(Some(min_conf), None, None, Some(true), None)
            .context("listunspent")
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

    pub fn load_wallet(&self) -> Result<()> {
        let root = self.root_client()?;
        if !self.wallet_exists_on_disk(&root)? {
            return Ok(());
        }

        let loaded_wallets: Vec<String> = root.list_wallets().context("list wallets")?;
        if loaded_wallets.contains(&self.watch_name) {
            return Ok(());
        }

        log::info!(
            "⏳ Loading watch-only wallet '{}' in Bitcoin Core (this may take a while)...",
            self.watch_name
        );

        let result: serde_json::Value = root
            .call::<serde_json::Value>("loadwallet", &[serde_json::json!(self.watch_name)])
            .with_context(|| format!("loadwallet '{}'", self.watch_name))?;

        let loaded_name = result
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if loaded_name != self.watch_name {
            return Err(anyhow::anyhow!(
                "loaded wallet mismatch: expected '{}', got '{}'",
                self.watch_name,
                loaded_name
            ));
        }

        if let Some(warning) = result
            .get("warning")
            .and_then(|value| value.as_str())
            .filter(|warning| !warning.is_empty())
        {
            return Err(anyhow::anyhow!("loadwallet warning: {warning}"));
        }

        Ok(())
    }

    pub fn unload_wallet(&self) -> Result<()> {
        let root = self.root_client()?;
        if !self.wallet_exists_on_disk(&root)? {
            return Ok(());
        }

        let loaded_wallets: Vec<String> = root.list_wallets().context("list wallets")?;
        if !loaded_wallets.contains(&self.watch_name) {
            return Ok(());
        }

        root.call::<serde_json::Value>("unloadwallet", &[serde_json::json!(self.watch_name)])
            .with_context(|| format!("unloadwallet '{}'", self.watch_name))?;

        Ok(())
    }

    pub fn loaded_wallets(&self) -> Result<Vec<String>> {
        let root = self.root_client()?;
        root.list_wallets().context("list wallets")
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
        let created_name = ans
            .get("name")
            .and_then(|value| value.as_str())
            .context("createwallet response missing name")?;
        if created_name != self.watch_name {
            return Err(anyhow::anyhow!(
                "Unexpected wallet name: expected '{}', got '{}'",
                self.watch_name,
                created_name
            ));
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

        let arr = ans
            .as_array()
            .context("importdescriptors response was not an array")?;
        if !arr
            .iter()
            .all(|e| e.get("success").and_then(|v| v.as_bool()) == Some(true))
        {
            let pretty = serde_json::to_string_pretty(&ans).unwrap_or_else(|_| format!("{ans:?}"));
            return Err(anyhow::anyhow!("Failed to import descriptors: {}", pretty));
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

    pub fn create_psbt_for_implicit_transfer(
        &self,
        token_inputs: &[OutPoint],
        target_address: &Address,
        amount_per_output: Amount,
        fee_rate: Option<f64>,
    ) -> Result<Psbt> {
        let client = self.watch_client()?;
        let (inputs, outputs, options) = walletcreatefundedpsbt_params_for_implicit_transfer(
            token_inputs,
            target_address,
            amount_per_output,
            fee_rate,
        );

        let funded: serde_json::Value = client
            .call(
                "walletcreatefundedpsbt",
                &[
                    inputs,
                    outputs,
                    serde_json::json!(0),
                    options,
                    serde_json::json!(true),
                ],
            )
            .context("walletcreatefundedpsbt (implicit transfer)")?;
        let psbt_b64 = funded["psbt"].as_str().context("psbt base64")?;
        let psbt: Psbt = psbt_b64.parse().context("parse psbt base64")?;
        Ok(psbt)
    }

    pub fn list_locked_unspent(&self) -> Result<BTreeSet<OutPoint>> {
        let client = self.watch_client()?;
        let locked: Vec<LockedOutpoint> = client
            .call("listlockunspent", &[])
            .context("listlockunspent")?;

        Ok(locked
            .into_iter()
            .map(|op| OutPoint {
                txid: op.txid,
                vout: op.vout,
            })
            .collect())
    }

    pub fn lock_unspent_outpoints(&self, outpoints: &[OutPoint]) -> Result<()> {
        if outpoints.is_empty() {
            return Ok(());
        }
        self.lockunspent(false, outpoints)
            .context("lock outpoints")?;
        Ok(())
    }

    pub fn unlock_unspent_outpoints(&self, outpoints: &[OutPoint]) -> Result<()> {
        if outpoints.is_empty() {
            return Ok(());
        }
        self.lockunspent(true, outpoints)
            .context("unlock outpoints")?;
        Ok(())
    }

    pub fn create_psbt_from_txout(&self, output: TxOut, fee_rate: Option<f64>) -> Result<Psbt> {
        let client = self.watch_client()?;

        let script = output.script_pubkey;
        let dummy = dummy_op_return_data_for_target_script_len(script.len())?;
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

    pub fn create_psbt_from_opreturn_and_payments(
        &self,
        op_return: TxOut,
        payments: Vec<(Address, Amount)>,
        fee_rate: Option<f64>,
    ) -> Result<Psbt> {
        let client = self.watch_client()?;

        let script = op_return.script_pubkey;
        let dummy = dummy_op_return_data_for_target_script_len(script.len())?;

        let mut outputs_vec = Vec::with_capacity(1 + payments.len());
        outputs_vec.push(serde_json::json!({ "data": dummy }));
        for (address, amount) in payments {
            outputs_vec.push(serde_json::json!({ address.to_string(): amount.to_btc() }));
        }
        let change_position = outputs_vec.len();
        let outputs = serde_json::Value::Array(outputs_vec);

        let mut options = serde_json::json!({});
        if let Some(fr) = fee_rate {
            options["fee_rate"] = serde_json::json!(fr);
        }
        // Keep all user-specified outputs at the front, so indices in the OP_RETURN mapping remain stable.
        options["changePosition"] = serde_json::json!(change_position);

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
            .context("walletcreatefundedpsbt (op_return + payments)")?;

        let psbt_b64 = funded["psbt"].as_str().context("psbt base64")?;
        let mut psbt: Psbt = psbt_b64.parse().context("parse psbt base64")?;

        psbt = substitute_first_opreturn_script(psbt, script).context("dummy not found")?;
        psbt = move_opreturn_first(psbt);
        Ok(psbt)
    }

    pub fn create_psbt_for_mix(
        &self,
        token_inputs: &[OutPoint],
        op_return: TxOut,
        payments: Vec<(Address, Amount)>,
        fee_rate: Option<f64>,
    ) -> Result<Psbt> {
        if token_inputs.is_empty() {
            return Err(anyhow::anyhow!("mix requires at least one input"));
        }

        let client = self.watch_client()?;

        let script = op_return.script_pubkey;
        let dummy = dummy_op_return_data_for_target_script_len(script.len())?;

        let mut outputs_vec = Vec::with_capacity(1 + payments.len());
        outputs_vec.push(serde_json::json!({ "data": dummy }));
        for (address, amount) in payments {
            outputs_vec.push(serde_json::json!({ address.to_string(): amount.to_btc() }));
        }
        let change_position = outputs_vec.len();
        let outputs = serde_json::Value::Array(outputs_vec);

        let inputs = serde_json::Value::Array(token_inputs.iter().map(outpoint_json).collect());

        let mut options = serde_json::json!({});
        if let Some(fr) = fee_rate {
            options["fee_rate"] = serde_json::json!(fr);
        }
        options["add_inputs"] = serde_json::json!(true);
        options["changePosition"] = serde_json::json!(change_position);

        let funded: serde_json::Value = client
            .call(
                "walletcreatefundedpsbt",
                &[
                    inputs,
                    outputs,
                    serde_json::json!(0),
                    options,
                    serde_json::json!(true),
                ],
            )
            .context("walletcreatefundedpsbt (mix)")?;

        let psbt_b64 = funded["psbt"].as_str().context("psbt base64")?;
        let mut psbt: Psbt = psbt_b64.parse().context("parse psbt base64")?;

        psbt = substitute_first_opreturn_script(psbt, script).context("dummy not found")?;
        psbt = move_opreturn_first(psbt);
        // Keep ownership inputs first so mix indexing can ignore trailing funding inputs.
        psbt = reorder_psbt_inputs(psbt, token_inputs)?;

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

    fn wallet_exists_on_disk(&self, root: &Client) -> Result<bool> {
        let response: serde_json::Value = root
            .call::<serde_json::Value>("listwalletdir", &[])
            .context("listwalletdir")?;
        let wallets = response
            .get("wallets")
            .and_then(|value| value.as_array())
            .ok_or_else(|| anyhow::anyhow!("unexpected listwalletdir response: {response:?}"))?;

        Ok(wallets.iter().any(|entry| {
            entry
                .get("name")
                .and_then(|value| value.as_str())
                .map(|name| self.watch_name == name)
                .unwrap_or(false)
        }))
    }

    fn lockunspent(&self, unlock: bool, outpoints: &[OutPoint]) -> Result<()> {
        let client = self.watch_client()?;
        let ops = serde_json::Value::Array(outpoints.iter().map(outpoint_json).collect());
        let ok: bool = client
            .call("lockunspent", &[serde_json::json!(unlock), ops])
            .context("lockunspent")?;
        if !ok {
            return Err(anyhow::anyhow!("lockunspent returned false"));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct LockedOutpoint {
    txid: bitcoin::Txid,
    vout: u32,
}

fn outpoint_json(outpoint: &OutPoint) -> serde_json::Value {
    serde_json::json!({
        "txid": outpoint.txid.to_string(),
        "vout": outpoint.vout,
    })
}

fn walletcreatefundedpsbt_params_for_implicit_transfer(
    token_inputs: &[OutPoint],
    target_address: &Address,
    amount_per_output: Amount,
    fee_rate: Option<f64>,
) -> (serde_json::Value, serde_json::Value, serde_json::Value) {
    let inputs = serde_json::Value::Array(token_inputs.iter().map(outpoint_json).collect());

    let mut outputs_vec = Vec::with_capacity(token_inputs.len());
    for _ in token_inputs {
        outputs_vec.push(serde_json::json!({
            target_address.to_string(): amount_per_output.to_btc()
        }));
    }
    let change_position = outputs_vec.len();
    let outputs = serde_json::Value::Array(outputs_vec);

    let mut options = serde_json::json!({});
    if let Some(fr) = fee_rate {
        options["fee_rate"] = serde_json::json!(fr);
    }
    // We preselect the token UTXOs as mandatory inputs, but still need Core to add extra inputs
    // (regular BTC UTXOs) to fund fees.
    options["add_inputs"] = serde_json::json!(true);
    // Ensure change is appended after all asset-carrying outputs so implicit transfer mapping
    // sends the NFTs to the requested address outputs.
    options["changePosition"] = serde_json::json!(change_position);

    (inputs, outputs, options)
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

fn reorder_psbt_inputs(mut psbt: Psbt, desired: &[OutPoint]) -> Result<Psbt> {
    let tx_inputs = &psbt.unsigned_tx.input;
    if tx_inputs.len() < desired.len() {
        return Err(anyhow::anyhow!(
            "psbt input count {} does not include expected {}",
            tx_inputs.len(),
            desired.len()
        ));
    }

    if psbt.inputs.len() != tx_inputs.len() {
        return Err(anyhow::anyhow!(
            "psbt metadata input count {} does not match tx inputs {}",
            psbt.inputs.len(),
            tx_inputs.len()
        ));
    }

    let mut used = vec![false; tx_inputs.len()];
    let mut new_inputs = Vec::with_capacity(tx_inputs.len());
    let mut new_meta = Vec::with_capacity(tx_inputs.len());

    for outpoint in desired {
        let idx = tx_inputs
            .iter()
            .position(|input| input.previous_output == *outpoint)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "psbt missing expected input {}:{}",
                    outpoint.txid,
                    outpoint.vout
                )
            })?;
        used[idx] = true;
        new_inputs.push(tx_inputs[idx].clone());
        new_meta.push(psbt.inputs[idx].clone());
    }

    for (idx, input) in tx_inputs.iter().enumerate() {
        if used[idx] {
            continue;
        }
        new_inputs.push(input.clone());
        new_meta.push(psbt.inputs[idx].clone());
    }

    psbt.unsigned_tx.input = new_inputs;
    psbt.inputs = new_meta;
    Ok(psbt)
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

fn dummy_op_return_data_for_target_script_len(target_len: usize) -> Result<ScriptBuf> {
    // Standard relay policy defaults: OP_RETURN outputs are limited by `-datacarriersize` (83 bytes).
    // We build a dummy `{"data": ...}` output whose script size is >= the final script size so that
    // fee estimation is conservative, while keeping it within standard limits.
    const MAX_NULL_DATA_SCRIPT_LEN: usize = 83;
    const MAX_NULL_DATA_LEN: usize = 80; // 1 (OP_RETURN) + 2 (PUSHDATA1) + 80 = 83

    if target_len > MAX_NULL_DATA_SCRIPT_LEN {
        return Err(anyhow::anyhow!(
            "op_return script too large: {} bytes (max standard {})",
            target_len,
            MAX_NULL_DATA_SCRIPT_LEN
        ));
    }

    for data_len in 0..=MAX_NULL_DATA_LEN {
        let script_len = 1 + pushdata_prefix_len(data_len) + data_len;
        if script_len >= target_len && script_len <= MAX_NULL_DATA_SCRIPT_LEN {
            return Ok(ScriptBuf::from(vec![0u8; data_len]));
        }
    }

    Err(anyhow::anyhow!(
        "unable to construct dummy op_return data for target script length {target_len}"
    ))
}

fn pushdata_prefix_len(data_len: usize) -> usize {
    match data_len {
        0..=75 => 1,      // OP_PUSHBYTES_N
        76..=255 => 2,    // OP_PUSHDATA1 + len (u8)
        256..=65535 => 3, // OP_PUSHDATA2 + len (u16)
        _ => 5,           // OP_PUSHDATA4 + len (u32)
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
    use std::str::FromStr;

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
    fn walletcreatefundedpsbt_params_for_implicit_transfer_sets_change_position() {
        let token_inputs = vec![
            OutPoint {
                txid: bitcoin::Txid::from_str(&"00".repeat(32)).unwrap(),
                vout: 1,
            },
            OutPoint {
                txid: bitcoin::Txid::from_str(&"11".repeat(32)).unwrap(),
                vout: 7,
            },
        ];
        let address = Address::from_str("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh")
            .unwrap()
            .require_network(Network::Bitcoin)
            .unwrap();

        let (inputs, outputs, options) = walletcreatefundedpsbt_params_for_implicit_transfer(
            &token_inputs,
            &address,
            Amount::from_sat(546),
            Some(12.3),
        );

        assert_eq!(inputs.as_array().unwrap().len(), 2);
        assert_eq!(outputs.as_array().unwrap().len(), 2);
        assert_eq!(options["changePosition"].as_u64().unwrap(), 2);
        assert_eq!(options["fee_rate"].as_f64().unwrap(), 12.3);
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

    #[test]
    fn load_wallet_loads_unloaded_wallet() {
        let node = corepc_node::Node::from_downloaded().unwrap();
        let auth = bitcoincore_rpc::Auth::CookieFile(node.params.cookie_file.clone());
        let node_url = url::Url::parse(&node.rpc_url()).unwrap();
        let wallet = create_wallet();
        let watch_name = "watch_only";
        let remote_wallet = RemoteWallet::new(watch_name.to_string(), &node_url, auth.clone());
        let external = wallet.public_descriptor(KeychainKind::External).to_string();
        let internal = wallet.public_descriptor(KeychainKind::Internal).to_string();
        remote_wallet
            .setup(external, internal)
            .expect("setup watch-only wallet");

        remote_wallet.unload_wallet().expect("unload wallet");

        let root = bitcoincore_rpc::Client::new(&node.rpc_url(), auth.clone()).unwrap();
        let loaded_wallets = root.list_wallets().expect("list wallets");
        assert!(
            !loaded_wallets.contains(&watch_name.to_string()),
            "wallet should be unloaded before load_wallet call"
        );

        remote_wallet.load_wallet().expect("load wallet");

        let loaded_wallets = root.list_wallets().expect("list wallets");
        assert!(
            loaded_wallets.contains(&watch_name.to_string()),
            "load_wallet should load the wallet into Core"
        );
    }
}
