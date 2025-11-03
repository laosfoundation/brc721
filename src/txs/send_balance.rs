use anyhow::Result;
use bitcoin::absolute::LockTime;
use bitcoin::transaction::Version;
use bitcoin::{psbt::Psbt, Address, Amount, ScriptBuf, Transaction, TxOut};

/// Build a minimal PSBT for sending a specific amount to the target address.
/// This constructs an unsigned transaction (no inputs yet) with a single
/// recipient output. Funding, signing, and broadcasting are handled elsewhere.
pub fn send_amount(
    target_address: Address,
    amount: Amount,
    _fee_rate: Option<f64>,
) -> Result<Psbt> {
    // Minimal unsigned tx: no inputs, one output to target. LockTime zero, v2.
    let script: ScriptBuf = target_address.script_pubkey();
    let tx = Transaction {
        version: Version(2),
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![TxOut {
            value: amount,
            script_pubkey: script,
        }],
    };

    // Wrap into a PSBT to be later funded (walletcreatefundedpsbt) and signed.
    let psbt = Psbt::from_unsigned_tx(tx)?;

    Ok(psbt)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use bitcoin::Network;

    #[test]
    fn check_invalid_address_network() {
        let unchecked_addr =
            Address::from_str("mvbnrCX3bg1cDRUu8pkecrvP6vQkSLDSou").expect("valid address format");
        assert!(
            unchecked_addr
                .clone()
                .require_network(Network::Regtest)
                .is_ok(),
            "address should be valid for regtest"
        );

        assert!(
            unchecked_addr.require_network(Network::Bitcoin).is_err(),
            "Regtest address should be valid for mainnet network"
        );
    }

    #[test]
    fn usage_example_send_amount() {
        // Intentional failing test: command not implemented yet
        let target_unchecked =
            Address::from_str("mvbnrCX3bg1cDRUu8pkecrvP6vQkSLDSou").expect("address");
        let target = target_unchecked
            .require_network(Network::Regtest)
            .expect("network");
        let amount = Amount::from_sat(10_000);
        let res = send_amount(target, amount, Some(2.5));
        assert!(res.is_ok());
    }
}
