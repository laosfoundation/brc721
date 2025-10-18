use bitcoincore_rpc::{Client, RpcApi};

pub fn send_register_collection(
    client: &Client,
    laos_hex: &str,
    rebaseable: bool,
    fee_rate_sat_vb: Option<f64>,
) -> Result<bitcoin::Txid, String> {
    use bitcoin::{
        absolute::LockTime,
        consensus::encode,
        transaction::Version as TxVersion,
        Amount, Transaction, TxOut,
    };
    use bitcoin::blockdata::script::Builder as ScriptBuilder;
    use bitcoin::script::PushBytesBuf;
    use crate::types::{BRC721_CODE, Brc721Command};

    let cleaned = laos_hex.trim_start_matches("0x");
    let addr_bytes = hex::decode(cleaned).map_err(|e| format!("invalid laos-hex: {}", e))?;
    if addr_bytes.len() != 20 {
        return Err("laos-hex must be exactly 20 bytes (40 hex chars)".to_string());
    }

    let mut payload = Vec::with_capacity(1 + 20 + 1);
    payload.push(Brc721Command::RegisterCollection as u8);
    payload.extend_from_slice(&addr_bytes);
    payload.push(if rebaseable { 1 } else { 0 });

    let push = PushBytesBuf::try_from(payload).map_err(|_| "invalid payload".to_string())?;

    let script = ScriptBuilder::new()
        .push_opcode(bitcoin::blockdata::opcodes::all::OP_RETURN)
        .push_opcode(BRC721_CODE)
        .push_slice(push)
        .into_script();

    let tx = Transaction {
        version: TxVersion::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![TxOut {
            value: Amount::from_sat(0),
            script_pubkey: script,
        }],
    };

    let hex = encode::serialize_hex(&tx);

    let mut options = bitcoincore_rpc::json::FundRawTransactionOptions {
        add_inputs: Some(true),
        include_watching: Some(true),
        lock_unspents: Some(true),
        replaceable: Some(true),
        ..Default::default()
    };
    if let Some(fr) = fee_rate_sat_vb {
        if fr > 0.0 {
            let btc_per_kb = fr * 1e-5f64;
            if let Ok(amount) = bitcoin::Amount::from_btc(btc_per_kb) {
                options.fee_rate = Some(amount);
            }
        }
    }

    let funded = client
        .fund_raw_transaction(hex, Some(&options), Some(false))
        .map_err(|e| format!("fundrawtransaction error: {}", e))?;

    let funded_tx = funded
        .transaction()
        .map_err(|e| format!("failed to decode funded tx: {}", e))?;

    let signed = client
        .sign_raw_transaction_with_wallet(&funded_tx, None, None)
        .map_err(|e| format!("signrawtransactionwithwallet error: {}", e))?;

    if !signed.complete {
        return Err("signing incomplete".to_string());
    }

    let signed_tx = signed
        .transaction()
        .map_err(|e| format!("failed to decode signed tx: {}", e))?;

    let txid = client
        .send_raw_transaction(&signed_tx)
        .map_err(|e| format!("sendrawtransaction error: {}", e))?;

    Ok(txid)
}
