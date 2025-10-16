use bitcoin::ScriptBuf;

use crate::types::RegisterCollectionPayload;

pub fn digest(script: &ScriptBuf) {
    parse(script);
}

fn parse(script: &ScriptBuf) -> Option<RegisterCollectionPayload> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CollectionAddress;
    use std::str::FromStr;

    #[test]
    fn test_parse_register_collection_no_rebaseable() {
        let script =
            ScriptBuf::from_hex("6a5f0014ffff0123ffffffffffffffffffffffff3210ffff00").unwrap();

        let r = parse(&script);
        assert!(r.is_some());
        let register_collection = r.unwrap();
        assert_eq!(
            register_collection.collection_address,
            CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
        );
        assert!(!register_collection.rebaseable)
    }

    #[test]
    fn test_parse_register_collection_rebaseable() {
        let script =
            ScriptBuf::from_hex("6a5f0014ffff0123ffffffffffffffffffffffff3210ffff01").unwrap();

        let r = parse(&script);
        assert!(r.is_some());
        let register_collection = r.unwrap();
        assert_eq!(
            register_collection.collection_address,
            CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
        );
        assert!(register_collection.rebaseable)
    }
}
