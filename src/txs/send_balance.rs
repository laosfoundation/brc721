use anyhow::Result;
use bitcoin::{Address, Amount};

/// Placeholder for `tx send-amount` implementation.
/// Will send the specified amount to the given target address.
pub fn send_amount(_target_address: Address, _amount: Amount, _fee_rate: Amount) -> Result<()> {
    // Intentionally unimplemented for now: we first analyze usage via tests
    // and then implement the logic (build, sign and broadcast).
    anyhow::bail!("not implemented")
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use bitcoin::Network;

    #[test]
    fn usage_example_send_amount() {
        // Intentional failing test: command not implemented yet
        let target_unchecked =
            Address::from_str("tb1qpdw3d0dcx5f274zmqsuzqjv0qlx2c9tq6n7y7x").expect("address");
        let target = target_unchecked
            .require_network(Network::Testnet)
            .expect("network");
        let amount = Amount::from_sat(10_000);
        let fee = Amount::from_sat(25);
        let res = send_amount(target, amount, fee);
        // We expect success here to document intended usage; currently unimplemented so this will fail
        assert!(res.is_ok(), "send_amount should succeed once implemented");
    }
}
