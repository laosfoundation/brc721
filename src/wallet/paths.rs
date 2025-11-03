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

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::Network;
    use std::path::PathBuf;

    #[test]
    fn test_wallet_db_path_mainnet() {
        let dir = "/tmp/testdata";
        let expected = PathBuf::from("/tmp/testdata/wallet-mainnet.sqlite");
        let path = wallet_db_path(dir, Network::Bitcoin);
        assert_eq!(path, expected);
    }

    #[test]
    fn test_wallet_db_path_testnet() {
        let dir = "/tmp/testdata";
        let expected = PathBuf::from("/tmp/testdata/wallet-testnet.sqlite");
        let path = wallet_db_path(dir, Network::Testnet);
        assert_eq!(path, expected);
    }

    #[test]
    fn test_wallet_db_path_signet() {
        let dir = "/tmp/testdata";
        let expected = PathBuf::from("/tmp/testdata/wallet-signet.sqlite");
        let path = wallet_db_path(dir, Network::Signet);
        assert_eq!(path, expected);
    }

    #[test]
    fn test_wallet_db_path_regtest() {
        let dir = "/tmp/testdata";
        let expected = PathBuf::from("/tmp/testdata/wallet-regtest.sqlite");
        let path = wallet_db_path(dir, Network::Regtest);
        assert_eq!(path, expected);
    }
}
