use bitcoin::Network;
use std::path::{Path, PathBuf};

pub fn wallet_db_path<P: AsRef<Path>>(data_dir: P, network: Network) -> PathBuf {
    let mut p = PathBuf::from(data_dir.as_ref());
    let name = format!(
        "wallet-{}.sqlite",
        match network {
            Network::Bitcoin => "mainnet",
            Network::Testnet => "testnet",
            Network::Signet => "signet",
            Network::Regtest => "regtest",
            _ => "unknown",
        }
    );
    p.push(name);
    p
}
