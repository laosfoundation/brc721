use std::path::{Path, PathBuf};

pub fn wallet_db_path<P: AsRef<Path>>(data_dir: P) -> PathBuf {
    let mut p = PathBuf::from(data_dir.as_ref());
    p.push("wallet.sqlite");
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_wallet_db_path_mainnet() {
        let dir = "/tmp/testdata";
        let expected = PathBuf::from("/tmp/testdata/wallet.sqlite");
        let path = wallet_db_path(dir);
        assert_eq!(path, expected);
    }
}
