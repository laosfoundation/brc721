use bitcoin::ScriptBuf;

pub fn digest(script: &ScriptBuf) {}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use bitcoin::ScriptBuf;
//     use std::str::FromStr;
//
//
//     #[test]
//     fn test_parse_register_collection_no_rebaseable() {
//         let txout = bitcoin::TxOut {
//             value: bitcoin::Amount::from_sat(0),
//             script_pubkey: ScriptBuf::from_hex(
//                 "6a5f0014ffff0123ffffffffffffffffffffffff3210ffff00",
//             )
//             .unwrap(),
//         };
//
//         let r = parse_register_output0(&txout);
//         assert!(r.is_some());
//         let register_collection = r.unwrap();
//         assert_eq!(
//             register_collection.collection_address,
//             CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
//         );
//         assert!(!register_collection.rebaseable)
//     }
//
//     #[test]
//     fn test_parse_register_collection_rebaseable() {
//         let txout = bitcoin::TxOut {
//             value: bitcoin::Amount::from_sat(0),
//             script_pubkey: ScriptBuf::from_hex(
//                 "6a5f0014ffff0123ffffffffffffffffffffffff3210ffff01",
//             )
//             .unwrap(),
//         };
//
//         let r = parse_register_output0(&txout);
//         assert!(r.is_some());
//         let register_collection = r.unwrap();
//         assert_eq!(
//             register_collection.collection_address,
//             CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
//         );
//         assert!(register_collection.rebaseable)
//     }
// }
