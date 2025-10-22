use bitcoin::Network;

pub fn parse_network<S: AsRef<str>>(s: Option<S>) -> Network {
    match s.as_ref().map(|x| x.as_ref().to_lowercase()) {
        Some(n) if n == "mainnet" || n == "bitcoin" => Network::Bitcoin,
        Some(n) if n == "testnet" => Network::Testnet,
        Some(n) if n == "signet" => Network::Signet,
        Some(n) if n == "regtest" => Network::Regtest,
        _ => Network::Regtest,
    }
}
