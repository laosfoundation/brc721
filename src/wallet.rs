use bitcoin::{consensus::encode, Amount, ScriptBuf, Transaction, TxOut};
use bitcoincore_rpc::{Client, RpcApi};
use serde_json::json;

pub struct Wallet;

impl Wallet {
    pub fn create_wallet(client: &Client, wallet_name: &str) -> bitcoincore_rpc::Result<()> {
        let _ = client.create_wallet(wallet_name, None, None, None, None);
        Ok(())
    }

    pub fn new_address(wallet_client: &Client) -> bitcoincore_rpc::Result<String> {
        wallet_client
            .get_new_address(None, None)
            .map(|a| a.assume_checked().to_string())
    }

    pub fn balance(wallet_client: &Client) -> bitcoincore_rpc::Result<f64> {
        wallet_client
            .get_balance(None, None)
            .map(|amt| amt.to_btc())
    }

    pub fn create_and_broadcast_collection(
        wallet_client: &Client,
        laos20: [u8; 20],
        rebaseable: bool,
        fee_rate_sat_vb: Option<f64>,
    ) -> bitcoincore_rpc::Result<String> {
        // Build OP_RETURN script: OP_RETURN OP_15 0x00 <20b laos> <rebaseable flag>
        let mut script = ScriptBuf::builder();
        script = script.push_opcode(bitcoin::opcodes::all::OP_RETURN);
        script = script.push_opcode(bitcoin::opcodes::all::OP_PUSHNUM_15);
        script = script.push_opcode(bitcoin::opcodes::all::OP_PUSHBYTES_0);
        script = script.push_slice(laos20);
        if rebaseable {
            script = script.push_opcode(bitcoin::opcodes::all::OP_PUSHNUM_1);
        } else {
            script = script.push_opcode(bitcoin::opcodes::all::OP_PUSHBYTES_0);
        }
        let opret = script.into_script();

        // Create a raw tx with a single OP_RETURN output of 0 sats (wallet will fund and add change)
        let tx = Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![],
            output: vec![TxOut {
                value: Amount::from_sat(0),
                script_pubkey: opret,
            }],
        };
        let raw_hex = hex::encode(encode::serialize(&tx));

        // Fund the transaction
        let mut opts = serde_json::Map::new();
        opts.insert("add_inputs".to_string(), json!(true));
        opts.insert("replaceable".to_string(), json!(true));
        if let Some(fr) = fee_rate_sat_vb {
            opts.insert("fee_rate".to_string(), json!(fr));
        }
        let funded: serde_json::Value =
            wallet_client.call("fundrawtransaction", &[json!(raw_hex), json!(opts)])?;
        let funded_hex = funded["hex"].as_str().expect("funded hex").to_string();

        // Sign with wallet
        let signed: serde_json::Value =
            wallet_client.call("signrawtransactionwithwallet", &[json!(funded_hex)])?;
        let signed_hex = signed["hex"].as_str().expect("signed hex").to_string();

        // Broadcast
        let txid: String = wallet_client.call("sendrawtransaction", &[json!(signed_hex)])?;
        Ok(txid)
    }
}
