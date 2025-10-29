use bitcoin::Network;
use tempfile::TempDir;

use crate::wallet::Wallet;

// #[test]
// fn test_wallet_creation() {
//     let data_dir = TempDir::new().expect("temp dir");
//     let wallet = Wallet::new(data_dir.path(), Network::Regtest);
//
//
//     let ans = wallet.init(None, None).expect("wallet");
//     assert!(ans.created);
//     wallet.setup_watchonly(rpc_url, auth, wallet_name, rescan)
// }
