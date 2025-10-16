use bitcoin::blockdata::opcodes::all as opcodes;
use bitcoin::ScriptBuf;

use crate::types::{Brc721Command, CollectionAddress, RegisterCollectionPayload, BRC721_CODE};

pub fn digest(script: &ScriptBuf) {
    parse(script);
}

fn parse(script: &ScriptBuf) -> Option<RegisterCollectionPayload> {
    let bytes = script.clone().into_bytes();

    if bytes.len() < 1 + 1 + 1 + 1 + 20 + 1 {
        return None;
    }

    if bytes[0] != opcodes::OP_RETURN.to_u8() {
        return None;
    }

    if bytes[1] != BRC721_CODE {
        return None;
    }

    if bytes[2] != Brc721Command::CreateCollection as u8 {
        return None;
    }

    let mut idx = 3;

    if bytes[idx] != 0x14 {
        return None;
    }
    idx += 1;

    if idx + 20 > bytes.len() {
        return None;
    }

    let addr_bytes = &bytes[idx..idx + 20];
    idx += 20;

    if idx >= bytes.len() {
        return None;
    }

    let rebase_flag = bytes[idx];
    let rebaseable = match rebase_flag {
        0 => false,
        1 => true,
        _ => return None,
    };

    let collection_address = CollectionAddress::from_slice(addr_bytes);

    Some(RegisterCollectionPayload {
        collection_address,
        rebaseable,
    })
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
