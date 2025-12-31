use bitcoincore_rpc::{Auth, Client, RpcApi};
use bitcoin::blockdata::script::Instruction;
use bitcoin::consensus::encode::deserialize;
use bitcoin::opcodes;
use bitcoin::{Address, Transaction, Txid};
use serde_json::json;
use std::process::Output;
use std::str::FromStr;
use tempfile::TempDir;
use testcontainers::runners::SyncRunner;

mod common;

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn combined_output(output: &Output) -> String {
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    format!("{}{}", out, err)
}

fn parse_txid(output: &Output) -> Txid {
    let combined = combined_output(output);
    for line in combined.lines() {
        if line.contains("txid:") {
            let txid_str = line
                .split_whitespace()
                .last()
                .expect("txid token at end of line");
            return txid_str.parse().expect("txid");
        }
    }
    panic!("txid not found in output:\n{}", combined);
}

fn parse_owner_output_address(output: &Output) -> Address {
    let combined = combined_output(output);
    for line in combined.lines() {
        if let Some(pos) = line.find("owner_output=") {
            let rest = &line[pos + "owner_output=".len()..];
            let end = rest.find(',').unwrap_or(rest.len());
            let addr_str = rest[..end].trim();
            return Address::from_str(addr_str)
                .expect("owner address")
                .assume_checked();
        }
    }
    panic!("owner_output not found in output:\n{}", combined);
}

fn collection_id_for_confirmed_tx(root: &Client, txid: &Txid) -> (u64, u32) {
    let txid_str = txid.to_string();
    let tx_verbose: serde_json::Value = root
        .call("getrawtransaction", &[json!(txid_str), json!(true)])
        .expect("getrawtransaction verbose");
    let blockhash = tx_verbose
        .get("blockhash")
        .and_then(|v| v.as_str())
        .expect("blockhash present after confirmation");

    let block: serde_json::Value = root
        .call("getblock", &[json!(blockhash), json!(1)])
        .expect("getblock");

    let height = block
        .get("height")
        .and_then(|v| v.as_u64())
        .expect("block height");

    let txs = block
        .get("tx")
        .and_then(|v| v.as_array())
        .expect("tx array");
    let tx_index = txs
        .iter()
        .position(|v| v.as_str() == Some(txid_str.as_str()))
        .expect("txid must be in its block") as u32;

    (height, tx_index)
}

#[test]
fn e2e_register_ownership_broadcasts_and_has_expected_outputs() {
    let image = common::bitcoind_image();
    let container = image.start().expect("start bitcoind container");
    let rpc_url = common::rpc_url(&container);
    let auth = Auth::UserPass("dev".into(), "dev".into());

    let root_client = Client::new(&rpc_url, auth.clone()).expect("rpc client initial");

    // Wallet: create and fund to pay fees
    let data_dir = TempDir::new().expect("temp dir");
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("wallet")
        .arg("init")
        .arg("--passphrase")
        .arg("passphrase")
        .arg("--mnemonic")
        .arg(MNEMONIC)
        .output()
        .expect("run wallet init");
    assert!(output.status.success(), "{:?}", output);

    let addr = common::wallet_address(&rpc_url, &data_dir);

    // Fund wallet so it can broadcast
    root_client.generate_to_address(101, &addr).expect("mine");

    // Register a collection so we can use a real collection id (HEIGHT:TX_INDEX)
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("tx")
        .arg("register-collection")
        .arg("--evm-collection-address")
        .arg("0xffff0123ffffffffffffffffffffffff3210ffff")
        .arg("--passphrase")
        .arg("passphrase")
        .output()
        .expect("run tx register-collection");
    assert!(output.status.success(), "{:?}", output);
    let collection_txid = parse_txid(&output);

    // Mine a block to confirm the collection tx, then compute (height, tx_index)
    root_client
        .generate_to_address(1, &addr)
        .expect("mine confirm collection");
    let (collection_height, collection_tx_index) =
        collection_id_for_confirmed_tx(&root_client, &collection_txid);
    let collection_id = format!("{collection_height}:{collection_tx_index}");

    // Send register-ownership
    let output = common::base_cmd(&rpc_url, &data_dir)
        .arg("tx")
        .arg("register-ownership")
        .arg("--collection-id")
        .arg(collection_id)
        .arg("--slots")
        .arg("0..=9,42,10..=19")
        .arg("--passphrase")
        .arg("passphrase")
        .output()
        .expect("run tx register-ownership");
    assert!(output.status.success(), "{:?}", output);
    let ownership_txid = parse_txid(&output);
    let owner_address = parse_owner_output_address(&output);

    // Ensure the tx is in mempool (broadcast succeeded)
    let _: serde_json::Value = root_client
        .call("getmempoolentry", &[json!(ownership_txid.to_string())])
        .expect("mempool entry exists");

    // Fetch and decode the broadcast tx, then assert output ordering:
    // vout0 = OP_RETURN (BRC-721), vout1 = spendable ownership output.
    let raw_hex: String = root_client
        .call("getrawtransaction", &[json!(ownership_txid.to_string())])
        .expect("getrawtransaction");
    let raw = hex::decode(raw_hex).expect("raw tx hex");
    let tx: Transaction = deserialize(&raw).expect("decode tx");

    assert!(
        tx.output.len() >= 2,
        "expected at least 2 outputs (op_return + ownership), got {}",
        tx.output.len()
    );

    let out0 = &tx.output[0];
    assert!(out0.script_pubkey.is_op_return(), "vout0 must be OP_RETURN");

    // Basic BRC-721 script shape: OP_RETURN OP_15 <pushbytes payload>
    let mut instructions = out0.script_pubkey.instructions();
    assert!(
        matches!(
            instructions.next(),
            Some(Ok(Instruction::Op(opcodes::all::OP_RETURN)))
        ),
        "script[0] must be OP_RETURN"
    );
    assert!(
        matches!(
            instructions.next(),
            Some(Ok(Instruction::Op(opcodes::all::OP_PUSHNUM_15)))
        ),
        "script[1] must be OP_15"
    );
    let payload = match instructions.next() {
        Some(Ok(Instruction::PushBytes(bytes))) => bytes.as_bytes().to_vec(),
        other => panic!("expected pushbytes payload, got {:?}", other),
    };
    assert_eq!(
        payload.first().copied(),
        Some(0x01),
        "payload must be register-ownership (0x01)"
    );

    let out1 = &tx.output[1];
    assert!(
        !out1.script_pubkey.is_op_return(),
        "vout1 must be spendable"
    );
    assert_eq!(
        out1.script_pubkey,
        owner_address.script_pubkey(),
        "vout1 must pay to the owner_output address printed by the CLI"
    );
    assert_eq!(out1.value.to_sat(), 546, "ownership output amount");

    // Confirm it can be mined
    root_client
        .generate_to_address(1, &addr)
        .expect("mine confirm ownership");
    let tx_verbose: serde_json::Value = root_client
        .call(
            "getrawtransaction",
            &[json!(ownership_txid.to_string()), json!(true)],
        )
        .expect("getrawtransaction verbose after mining");
    let confirmations = tx_verbose
        .get("confirmations")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    assert!(confirmations >= 1, "expected confirmations >= 1");
}

