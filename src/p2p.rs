use bitcoin::p2p::Magic;

pub fn magic_from_network_name(name: &str) -> Magic {
    match name.to_lowercase().as_str() {
        "mainnet" | "bitcoin" => Magic::BITCOIN,
        "testnet" => Magic::TESTNET3,
        "signet" => Magic::SIGNET,
        "regtest" => Magic::REGTEST,
        _ => Magic::BITCOIN,
    }
}

pub fn default_p2p_port_for_network(name: &str) -> u16 {
    match name.to_lowercase().as_str() {
        "mainnet" | "bitcoin" => 8333,
        "testnet" => 18333,
        "signet" => 38333,
        "regtest" => 18444,
        _ => 8333,
    }
}
